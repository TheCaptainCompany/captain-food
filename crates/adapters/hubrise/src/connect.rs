//! HubRise **connect flow** (issue #20, `docs/integrations/hubrise-process.md` §0): the one-time (and
//! idempotently re-runnable) OAuth connection of a HubRise Account to Captain.Food.
//!
//! ```text
//! GET /adapters/hubrise/connect ──302──▶ HubRise authorize ──▶ GET /adapters/hubrise/oauth/callback?code=…
//!   callback → exchange code → token + connection scope (account_id, …)
//!            → pull /account, /locations, /catalogs
//!            → journaled WORKER sends: RegisterRestaurantAccount + RegisterRestaurant per location
//!              + CreateCatalog per catalog (all with the enricher's derived UUIDv5 ids)
//!            → persist the account-scoped token (hubrise_connections) + the location snapshot
//!            → initial ImportCatalog per catalog (so onboarding completes without waiting for a callback)
//! ```
//!
//! Design (ADR-20260721-100601):
//! - **No new domain messages.** Provisioning reuses the EXISTING rejectable commands with ids supplied
//!   by the ACL (commands.yaml: aggregate ids are client/ACL-generated). Creation handlers are
//!   idempotent on an existing id, so a re-connect adopts the aggregates it created before — a
//!   re-connect is a token refresh + location catch-up, never a duplicate.
//! - **The token is a credential, not a business fact**: it goes to `hubrise_connections`
//!   (adapter-owned, unreachable from GraphQL), never into `domain_events`.
//! - Every send goes through the WORKER-channel journaling dispatch (#15): `message_id` =
//!   UUIDv5(connect attempt, command type, entity id) — per-attempt, because two connects may
//!   legitimately re-send the same command with fresher HubRise data; `correlation_id` = the attempt id,
//!   so one connect's whole provisioning fans out under a single correlation.
//! - Deterministic REJECTIONS (e.g. a slug owned by another restaurant) are collected as warnings and
//!   never abort the connect — the SIRENE lesson: replaying a catalogued rejection is pure churn.

use std::sync::Arc;

use actor_client::mailbox::{Envelope, Mailbox};
use actor_client::{ActorClient, OperationStatusBus};
use actor_client::generated::actor_clients::{
    CatalogClient, RestaurantAccountClient, RestaurantClient,
};
use actor_client::EnqueueOutcome;
use application::queries::RestaurantReadRepository;
use domain::generated::commands::{CreateCatalog, RegisterRestaurant, RegisterRestaurantAccount};
use domain::generated::entities::{Address, TaxRate};
use domain::generated::scalars::{
    AddressLine, CatalogName, CityName, CountryCode,
    CurrencyCode, ExternalReference, PostalCode, RestaurantDisplayName, RestaurantLegalName,
    RestaurantListingStatus, TaxRatePercent, TimeZone,
};
use domain::shared::errors::DomainError;

use crate::api::TokenResponse;
use crate::connections::{ConnectedLocation, HubRiseConnection, HubRiseConnections};
use crate::enrich::{
    derive, derive_catalog_id, derive_restaurant_account_id, derive_restaurant_id,
    hubrise_system_user_id, map_catalog, EXTERNAL_USER_TYPE,
};

/// Default VAT for a freshly connected account: the French reduced rate for prepared food. The
/// account-level default only seeds `RestaurantAccount.defaultTaxRate` — the imported catalog carries
/// its own per-product HubRise tax rates.
const DEFAULT_TAX_PERCENT: f64 = 10.0;

// ================================================================================================
// Gateway — the outbound HubRise surface the connect flow needs, behind a trait for unit tests
// ================================================================================================

/// OAuth exchange + provisioning pulls. Implemented over [`crate::api`]; faked in tests.
#[async_trait::async_trait]
pub trait HubRiseConnectGateway: Send + Sync {
    async fn exchange_code(&self, code: &str) -> Result<TokenResponse, String>;
    async fn pull_account(&self, token: &str) -> Result<serde_json::Value, String>;
    async fn pull_locations(&self, token: &str) -> Result<serde_json::Value, String>;
    async fn pull_catalogs(&self, token: &str) -> Result<serde_json::Value, String>;
    async fn pull_catalog(&self, token: &str, catalog_id: &str) -> Result<serde_json::Value, String>;
}

/// The real gateway: [`crate::api::exchange_code`] with the app credentials + the token-per-call API.
pub struct HttpHubRiseConnectGateway {
    pub api: crate::api::HubRiseApi,
    pub client_id: String,
    pub client_secret: String,
}

#[async_trait::async_trait]
impl HubRiseConnectGateway for HttpHubRiseConnectGateway {
    async fn exchange_code(&self, code: &str) -> Result<TokenResponse, String> {
        crate::api::exchange_code(&self.client_id, &self.client_secret, code)
            .await
            .map_err(|e| e.to_string())
    }
    async fn pull_account(&self, token: &str) -> Result<serde_json::Value, String> {
        self.api.get_account(token).await.map_err(|e| e.to_string())
    }
    async fn pull_locations(&self, token: &str) -> Result<serde_json::Value, String> {
        self.api.get_locations(token).await.map_err(|e| e.to_string())
    }
    async fn pull_catalogs(&self, token: &str) -> Result<serde_json::Value, String> {
        self.api.get_catalogs(token).await.map_err(|e| e.to_string())
    }
    async fn pull_catalog(&self, token: &str, catalog_id: &str) -> Result<serde_json::Value, String> {
        self.api.get_catalog(token, catalog_id).await.map_err(|e| e.to_string())
    }
}

// ================================================================================================
// Wire types — the HubRise subset the connect flow reads (unknown fields ignored)
// ================================================================================================

#[derive(Debug, Clone, serde::Deserialize)]
struct HrAccount {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    currency: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct HrLocation {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    postal_code: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    country: Option<String>,
    /// Docs show `timezone` as an object with a `name`; tolerate a bare string too.
    #[serde(default)]
    timezone: Option<serde_json::Value>,
    #[serde(default)]
    preparation_time: Option<i64>,
}

impl HrLocation {
    fn timezone_name(&self) -> Option<String> {
        match &self.timezone {
            Some(serde_json::Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
            Some(serde_json::Value::Object(o)) => {
                o.get("name").and_then(|v| v.as_str()).map(str::to_string)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct HrCatalogHead {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    location_id: Option<String>,
}

/// HubRise list endpoints return bare arrays; tolerate a `{ "<key>": [...] }` wrapper defensively.
fn as_list<T: serde::de::DeserializeOwned>(json: &serde_json::Value, key: &str) -> Result<Vec<T>, String> {
    let val = json.get(key).cloned().unwrap_or_else(|| json.clone());
    serde_json::from_value(val).map_err(|e| format!("unexpected {key} list shape: {e}"))
}

// ================================================================================================
// Outcome
// ================================================================================================

/// What one connect (OAuth callback) did — surfaced on the callback response and logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectSummary {
    pub restaurant_account_id: uuid::Uuid,
    pub hubrise_account_id: String,
    pub account_name: Option<String>,
    pub locations: usize,
    pub catalogs_created: usize,
    pub catalogs_imported: usize,
    /// Deterministic rejections / unmappable entries — the connect still completed.
    pub warnings: Vec<String>,
}

/// Why a connect attempt failed outright (nothing usable was provisioned/stored).
#[derive(Debug)]
pub enum ConnectError {
    /// The code→token exchange failed (bad/expired code, wrong app credentials).
    Exchange(String),
    /// The token response names no account — the app's OAuth scope must include an account
    /// (`account[...]`) or location scope; a profile-only connection cannot be provisioned.
    NoAccountInScope,
    /// A provisioning pull failed (account/locations unreachable) — retry the connect.
    Pull(String),
    /// Journal/event-store failure — retry the connect.
    Infra(DomainError),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exchange(e) => write!(f, "token exchange failed: {e}"),
            Self::NoAccountInScope => {
                write!(f, "token response names no HubRise account (check the OAuth scope)")
            }
            Self::Pull(e) => write!(f, "hubrise provisioning pull failed: {e}"),
            Self::Infra(e) => write!(f, "provisioning write failed: {e}"),
        }
    }
}

// ================================================================================================
// The flow
// ================================================================================================

/// Object-safe façade for the HTTP shell (mirrors [`crate::enrich::Enricher`]).
#[async_trait::async_trait]
pub trait ConnectService: Send + Sync {
    async fn connect(&self, code: &str) -> Result<ConnectSummary, ConnectError>;
}

/// Drives one OAuth callback end-to-end. Generic over the gateway so the whole provisioning
/// (derived ids, journaling, idempotent re-connect) is unit-testable in memory.
pub struct HubRiseConnectFlow<G: HubRiseConnectGateway> {
    mailbox: Arc<dyn Mailbox>,
    /// The D4 READ door (#304): status is read through the one generic client, never off the port
    /// -- since #304 `Mailbox::by_message` demands a witness only `actor_client` can mint, so this
    /// is the compiler's choice as much as ours.
    status: ActorClient,
    restaurants: Arc<dyn RestaurantReadRepository>,
    connections: Arc<dyn HubRiseConnections>,
    gateway: G,
}

impl<G: HubRiseConnectGateway> HubRiseConnectFlow<G> {
    /// `bus` is the process-wide operation-response bus, or `None` in a deployment that has no
    /// shared one (a standalone adapter — `run_standalone_workers` publishes onto a bus of its
    /// own that nothing here can see). Taking the BUS rather than a ready-made [`ActorClient`]
    /// is deliberate: a caller cannot then hand us a read door built over a DIFFERENT mailbox
    /// than the one we write through, and the pull-only posture stays visible as `None`.
    ///
    /// Today both arms behave identically here — this flow only ever PULLS a terminal status
    /// (`await_message_terminal`), never `watch`. The distinction is forward-looking: it stops a
    /// `watch` added later from hanging in the standalone topology and nowhere else.
    pub fn new(
        mailbox: Arc<dyn Mailbox>,
        bus: Option<OperationStatusBus>,
        restaurants: Arc<dyn RestaurantReadRepository>,
        connections: Arc<dyn HubRiseConnections>,
        gateway: G,
    ) -> Self {
        let status = match bus {
            Some(bus) => ActorClient::new(mailbox.clone(), bus),
            None => ActorClient::pull_only(mailbox.clone()),
        };
        Self { mailbox, status, restaurants, connections, gateway }
    }

    /// The WORKER envelope for one provisioning send through a typed actor client (#284 slice 3).
    /// `message_id` is scoped to THIS attempt (a re-connect re-sends with fresher data under new
    /// ids; the aggregates' own idempotency absorbs replays), `correlation_id` groups the
    /// attempt's whole fan-out, and `cause_id` is the PARENT (the connect attempt) — the delivery
    /// side chains appended events to the message itself.
    fn envelope(attempt: uuid::Uuid, command_type: &str, entity: &str) -> Envelope {
        Envelope {
            message_id: derive("connect-command", &format!("{attempt}:{command_type}:{entity}")),
            correlation_id: attempt,
            cause_id: Some(attempt),
            session_id: None,
            trace_id: None,
            user_id: Some(hubrise_system_user_id()),
            user_type: EXTERNAL_USER_TYPE.to_string(),
            channel: "WORKER".into(),
        }
    }

    /// Interpret one fire-and-forget hand-off (ADR-20260731-122500): the mailbox worker delivers;
    /// rejections live on the mailbox row and the supervision lanes, not here. `Some(message_id)`
    /// = durably handed off (fresh or an idempotent replay); `None` = payload conflict (warned —
    /// never enqueued).
    fn handoff(
        command_type: &str,
        entity: &str,
        message_id: uuid::Uuid,
        outcome: EnqueueOutcome,
        warnings: &mut Vec<String>,
    ) -> Option<uuid::Uuid> {
        match outcome {
            EnqueueOutcome::Enqueued | EnqueueOutcome::Deduplicated(_) => Some(message_id),
            EnqueueOutcome::PayloadConflict(status) => {
                warnings.push(format!(
                    "{command_type} {entity}: payload conflict under a replayed id (mailbox row {status:?}) -- not re-sent"
                ));
                None
            }
        }
    }

    /// Await a sent command's TERMINAL mailbox status (bounded poll). The provisioning chain is
    /// causally ordered but its actors live on DIFFERENT lanes with no cross-lane ordering:
    /// `register_restaurant` folds the ACCOUNT stream (`RestaurantAccountNotFound`), so the
    /// account registration must be delivered before its dependents are enqueued — the old
    /// inline dispatch gave this ordering for free. Returns the terminal status, or `None` on
    /// timeout (the caller degrades to a warning; a re-connect replays idempotently).
    async fn await_message_terminal(
        &self,
        message_id: uuid::Uuid,
    ) -> Option<domain::generated::scalars::InboundMessageStatus> {
        use domain::generated::scalars::InboundMessageStatus as S;
        for _ in 0..40 {
            if let Ok(Some(row)) = self.status.get_operation_status(message_id).await {
                if !matches!(row.status, S::RECEIVED | S::SCHEDULED) {
                    return Some(row.status);
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        None
    }

    /// The registered Restaurant must be visible in the READ MODEL before `create_catalog` (its
    /// `RestaurantNotFound` guard reads the projection, which folds asynchronously) — poll briefly.
    async fn await_restaurant_projection(&self, restaurant_id: uuid::Uuid) -> bool {
        use domain::generated::scalars::RestaurantId;
        for _ in 0..40 {
            if let Ok(Some(_)) = self.restaurants.by_id(RestaurantId(restaurant_id)).await {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        false
    }

    pub async fn connect(&self, code: &str) -> Result<ConnectSummary, ConnectError> {
        let token = self.gateway.exchange_code(code).await.map_err(ConnectError::Exchange)?;
        let account_json =
            self.gateway.pull_account(&token.access_token).await.map_err(ConnectError::Pull)?;
        let account: HrAccount = serde_json::from_value(
            account_json.get("account").cloned().unwrap_or(account_json),
        )
        .map_err(|e| ConnectError::Pull(format!("unexpected account shape: {e}")))?;

        // The HubRise account id anchors every derived identity; the token response is authoritative,
        // the pulled account a fallback.
        let hubrise_account_id = token
            .account_id
            .clone()
            .or_else(|| account.id.clone())
            .ok_or(ConnectError::NoAccountInScope)?;
        let account_name = account.name.clone().or_else(|| token.account_name.clone());
        let restaurant_account_id = derive_restaurant_account_id(&hubrise_account_id).0;

        let locations_json =
            self.gateway.pull_locations(&token.access_token).await.map_err(ConnectError::Pull)?;
        let locations: Vec<HrLocation> =
            as_list(&locations_json, "locations").map_err(ConnectError::Pull)?;

        let mut warnings = Vec::new();
        // Catalogs are provisioned best-effort: a failed listing must not lose the token/connection.
        let catalogs: Vec<HrCatalogHead> = match self.gateway.pull_catalogs(&token.access_token).await
        {
            Ok(json) => as_list(&json, "catalogs").unwrap_or_else(|e| {
                warnings.push(e);
                vec![]
            }),
            Err(e) => {
                warnings.push(format!("catalog listing failed (connect still recorded): {e}"));
                vec![]
            }
        };

        let attempt = uuid::Uuid::new_v4();

        // 1) The account aggregate (idempotent on the derived id).
        let cmd = RegisterRestaurantAccount {
            restaurant_account_id: domain::generated::scalars::RestaurantAccountId(
                restaurant_account_id,
            ),
            legal_name: RestaurantLegalName(
                account_name.clone().unwrap_or_else(|| format!("HubRise {hubrise_account_id}")),
            ),
            contact: None, // HubRise exposes no account contact; completed manually (hubrise.md §4.1)
            default_currency: CurrencyCode(
                account.currency.clone().unwrap_or_else(|| "EUR".to_string()),
            ),
            default_tax_rate: TaxRate {
                delivery: TaxRatePercent(DEFAULT_TAX_PERCENT),
                collection: None,
                eat_in: None,
            },
            timezone: locations.first().and_then(|l| l.timezone_name()).map(TimeZone),
            r#ref: Some(ExternalReference(hubrise_account_id.clone())),
        };
        let env = Self::envelope(attempt, "RegisterRestaurantAccount", &hubrise_account_id);
        let message_id = env.message_id;
        let outcome = RestaurantAccountClient::new(self.mailbox.clone(), restaurant_account_id)
            .send(cmd, env)
            .await
            .map_err(ConnectError::Infra)?;
        let account_msg = Self::handoff(
            "RegisterRestaurantAccount",
            &hubrise_account_id,
            message_id,
            outcome,
            &mut warnings,
        );
        // The dependents fold the ACCOUNT stream on other lanes — deliver the account first
        // (cross-lane enqueue order guarantees nothing). A rejection/timeout degrades to a
        // warning the operator sees synchronously, the property the old inline dispatch had.
        if let Some(account_msg) = account_msg {
            use domain::generated::scalars::InboundMessageStatus as S;
            match self.await_message_terminal(account_msg).await {
                Some(S::SUCCEEDED | S::IGNORED | S::DUPLICATE) => {}
                Some(status) => warnings.push(format!(
                    "RegisterRestaurantAccount {hubrise_account_id}: delivered {status:?} — \
                     dependent registrations may reject (fix and re-connect)"
                )),
                None => warnings.push(format!(
                    "RegisterRestaurantAccount {hubrise_account_id}: not delivered before timeout — \
                     dependent registrations may land first and reject (re-connect replays them)"
                )),
            }
        }

        // 2) One Restaurant per location (the location IS the restaurant; ids reconcile with the
        //    enricher's derivation so later callbacks land on these aggregates).
        let mut connected_locations = Vec::with_capacity(locations.len());
        for loc in &locations {
            let restaurant_id = derive_restaurant_id(&loc.id);
            let name = loc.name.clone().unwrap_or_else(|| format!("Location {}", loc.id));
            // No derived slug (ADR-20260728-011344). A connected HubRise location is a real partner
            // onboarding, so its storefront address is the OWNER's to choose via
            // ConfigureRestaurantSlug — before activation, which is already a human decision here.
            // Inventing `bella-pizza-loc-1` would hand a merchant a hostname built from a HubRise
            // internal location id.
            let city = loc.city.clone().unwrap_or_default();
            let cmd = RegisterRestaurant {
                mode: None,
                restaurant_id,
                account_id: Some(domain::generated::scalars::RestaurantAccountId(
                    restaurant_account_id,
                )),
                // "Menu synced (e.g. HubRise) but no signed contract; not orderable" — exactly a
                // freshly connected account. Activation stays a human decision.
                listing_status: Some(RestaurantListingStatus::PASSIVE_PARTNER),
                display_name: RestaurantDisplayName(name),
                contact: None, // HubRise locations expose no email/phone (hubrise.md §4.1)
                website: None,
                tags: vec![],
                margin_rate: None,
                cuisine_category: None,
                uber_prices_opt_in: None,
                address: Address {
                    line1: AddressLine(
                        loc.address
                            .clone()
                            .filter(|a| !a.trim().is_empty())
                            .unwrap_or_else(|| city.clone()),
                    ),
                    line2: None,
                    postal_code: PostalCode(loc.postal_code.clone().unwrap_or_default()),
                    city: CityName(city),
                    country: CountryCode(loc.country.clone().unwrap_or_else(|| "FR".to_string())),
                },
                location: None,
                timezone: loc.timezone_name().map(TimeZone),
                preparation_time_minutes: loc.preparation_time,
                opening_hours: vec![], // wire shape unconfirmed — left for manual/API completion
                external_identifiers: vec![],
                r#ref: Some(ExternalReference(loc.id.clone())),
            };
            let env = Self::envelope(attempt, "RegisterRestaurant", &loc.id);
            let message_id = env.message_id;
            let outcome = RestaurantClient::new(self.mailbox.clone(), restaurant_id.0)
                .send(cmd, env)
                .await
                .map_err(ConnectError::Infra)?;
            let _ = Self::handoff("RegisterRestaurant", &loc.id, message_id, outcome, &mut warnings);
            connected_locations.push(ConnectedLocation {
                hubrise_location_id: loc.id.clone(),
                restaurant_account_id,
                restaurant_id: restaurant_id.0,
            });
        }

        // 3) Persist the connection BEFORE the catalog leg: from here on the enricher can resolve
        //    this account's token, even if the initial import below fails.
        self.connections
            .upsert(
                &HubRiseConnection {
                    restaurant_account_id,
                    hubrise_account_id: hubrise_account_id.clone(),
                    access_token: token.access_token.clone(),
                    account_name: account_name.clone(),
                },
                &connected_locations,
            )
            .await
            .map_err(ConnectError::Infra)?;

        // 4) Catalogs: CreateCatalog with the derived id, then an initial ImportCatalog so the menu
        //    is live without waiting for the first HubRise callback.
        let (mut created, mut imported) = (0usize, 0usize);
        for cat in &catalogs {
            // Our Catalog belongs to one Restaurant: the catalog's own location, else the account's
            // first (single-location accounts are the V0 norm; multi-location account catalogs warn).
            let Some(location_id) = cat
                .location_id
                .clone()
                .or_else(|| (locations.len() == 1).then(|| locations[0].id.clone()))
                .or_else(|| token.location_id.clone())
            else {
                warnings.push(format!(
                    "catalog {}: no owning location resolvable (account has {} locations) — skipped",
                    cat.id,
                    locations.len()
                ));
                continue;
            };
            let catalog_id = derive_catalog_id(&cat.id);
            let restaurant_id = derive_restaurant_id(&location_id);

            // `create_catalog` guards on the Restaurant READ MODEL; the projection folds async.
            if !self.await_restaurant_projection(restaurant_id.0).await {
                warnings.push(format!(
                    "catalog {}: restaurant projection for location {location_id} not visible yet — \
                     catalog not created (re-connect or wait for the next catalog callback)",
                    cat.id
                ));
                continue;
            }

            // No slug here: the catalog ROUTE is the owner's choice (ConfigureCatalogSlug), never
            // derived from an imported name. An import that invented one would pin a public path the
            // merchant never picked, and a HubRise rename would silently not move it.
            let cmd = CreateCatalog {
                catalog_id,
                restaurant_id,
                name: CatalogName(cat.name.clone().unwrap_or_else(|| "Menu".to_string())),
                r#ref: Some(ExternalReference(cat.id.clone())),
            };
            let env = Self::envelope(attempt, "CreateCatalog", &cat.id);
            let message_id = env.message_id;
            let outcome = CatalogClient::new(self.mailbox.clone(), catalog_id.0)
                .send(cmd, env)
                .await
                .map_err(ConnectError::Infra)?;
            let ok =
                Self::handoff("CreateCatalog", &cat.id, message_id, outcome, &mut warnings).is_some();
            if !ok {
                continue;
            }
            created += 1;

            // Initial import — same pull + ACL mapping the callback enrichment uses.
            match self.gateway.pull_catalog(&token.access_token, &cat.id).await {
                Ok(json) => match map_catalog(&json, &cat.id, &location_id) {
                    Ok(cmd) => {
                        let env = Self::envelope(attempt, "ImportCatalog", &cat.id);
                        let message_id = env.message_id;
                        // Same Catalog lane as CreateCatalog above — head-of-line order
                        // delivers the creation before the import.
                        let outcome = CatalogClient::new(self.mailbox.clone(), catalog_id.0)
                            .send(cmd, env)
                            .await
                            .map_err(ConnectError::Infra)?;
                        let ok =
                            Self::handoff("ImportCatalog", &cat.id, message_id, outcome, &mut warnings)
                                .is_some();
                        if ok {
                            imported += 1;
                        }
                    }
                    Err(e) => warnings.push(format!("catalog {}: initial import unmappable: {e}", cat.id)),
                },
                Err(e) => warnings.push(format!("catalog {}: initial pull failed: {e}", cat.id)),
            }
        }

        Ok(ConnectSummary {
            restaurant_account_id,
            hubrise_account_id,
            account_name,
            locations: connected_locations.len(),
            catalogs_created: created,
            catalogs_imported: imported,
            warnings,
        })
    }
}

#[async_trait::async_trait]
impl<G: HubRiseConnectGateway> ConnectService for HubRiseConnectFlow<G> {
    async fn connect(&self, code: &str) -> Result<ConnectSummary, ConnectError> {
        HubRiseConnectFlow::connect(self, code).await
    }
}

// ================================================================================================
// Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use actor_client::mailbox::mem::MemMailbox;
    use actor_client::mailbox::MailboxEntry;
    use application::queries::{RestaurantFilter, RestaurantRow};
    use domain::generated::scalars::{
        OrderAcceptanceMode, RestaurantId as RestaurantIdScalar, RestaurantStatus,
    };

    use crate::connections::mem::MemHubRiseConnections;

    // ----- fake restaurant read model: a caught-up projection (by_id resolves immediately) -----

    struct CaughtUpRestaurants;

    fn dummy_row(id: RestaurantIdScalar) -> RestaurantRow {
        RestaurantRow {
            slug: None,   // a connected location has no storefront address until the owner picks one
            restaurant_id: id,
            restaurant_account_id: None,
            listing_status: RestaurantListingStatus::PASSIVE_PARTNER,
            external_identifiers: None,
            google_place_id: None,
            display_name: RestaurantDisplayName("x".into()),
            description: None,
            tags: None,
            margin_rate: None,
            cuisine_category: None,
            uber_prices_opt_in: None,
            website: None,
            rating: None,
            reviews_count: None,
            gbp_order_url: None,
            gbp_link_status: None,
            address: serde_json::json!({}),
            location: None,
            opening_hours: serde_json::json!([]),
            status: RestaurantStatus::DRAFT,
            order_acceptance: OrderAcceptanceMode::NORMAL,
            default_currency: CurrencyCode("EUR".into()),
            timezone: None,
            preparation_time_minutes: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    // Only the test doubles need `Slug` now that the connect flow derives no storefront address
    // (ADR-20260728-011344) -- importing it at module level would warn in the lib build.
    use domain::generated::scalars::Slug;

    #[async_trait::async_trait]
    impl RestaurantReadRepository for CaughtUpRestaurants {
        async fn list(&self, _f: RestaurantFilter) -> Result<Vec<RestaurantRow>, DomainError> {
            Ok(vec![])
        }
        async fn by_slug(&self, _slug: Slug) -> Result<Option<RestaurantRow>, DomainError> {
            Ok(None) // no colliding slugs in these tests
        }
        async fn by_id(
            &self,
            id: RestaurantIdScalar,
        ) -> Result<Option<RestaurantRow>, DomainError> {
            Ok(Some(dummy_row(id)))
        }
    }

    // ----- fake gateway -----

    #[derive(Clone)]
    struct FakeGateway {
        token: TokenResponse,
        account: serde_json::Value,
        locations: serde_json::Value,
        catalogs: Result<serde_json::Value, String>,
        catalog_content: serde_json::Value,
    }

    fn token_response(access_token: &str) -> TokenResponse {
        serde_json::from_value(serde_json::json!({
            "access_token": access_token,
            "account_id": "acc_1",
            "account_name": "Bella Pizza",
        }))
        .unwrap()
    }

    fn fake_gateway(access_token: &str) -> FakeGateway {
        FakeGateway {
            token: token_response(access_token),
            account: serde_json::json!({ "id": "acc_1", "name": "Bella Pizza", "currency": "EUR" }),
            locations: serde_json::json!([{
                "id": "loc_1", "name": "Bella Pizza", "address": "3 rue Nationale",
                "postal_code": "37000", "city": "Tours", "country": "FR",
                "timezone": { "name": "Europe/Paris" }, "preparation_time": 15
            }]),
            catalogs: Ok(serde_json::json!([{ "id": "cat_1", "name": "Menu", "location_id": "loc_1" }])),
            catalog_content: serde_json::json!({
                "data": { "products": [{
                    "id": "p_1", "name": "Margherita",
                    "skus": [{ "id": "s_1", "ref": "SKU-MARG", "price": "9.80 EUR" }]
                }] }
            }),
        }
    }

    #[async_trait::async_trait]
    impl HubRiseConnectGateway for FakeGateway {
        async fn exchange_code(&self, code: &str) -> Result<TokenResponse, String> {
            assert_eq!(code, "the-code");
            Ok(self.token.clone())
        }
        async fn pull_account(&self, token: &str) -> Result<serde_json::Value, String> {
            assert_eq!(token, self.token.access_token);
            Ok(self.account.clone())
        }
        async fn pull_locations(&self, _token: &str) -> Result<serde_json::Value, String> {
            Ok(self.locations.clone())
        }
        async fn pull_catalogs(&self, _token: &str) -> Result<serde_json::Value, String> {
            self.catalogs.clone()
        }
        async fn pull_catalog(&self, _token: &str, id: &str) -> Result<serde_json::Value, String> {
            assert_eq!(id, "cat_1");
            Ok(self.catalog_content.clone())
        }
    }

    fn flow(
        connections: Arc<MemHubRiseConnections>,
        gateway: FakeGateway,
    ) -> (HubRiseConnectFlow<FakeGateway>, Arc<MemMailbox>) {
        // Instantly-delivered: the connect flow awaits the account leg's terminal status.
        let mailbox = Arc::new(MemMailbox::instantly_delivered());
        (
            HubRiseConnectFlow::new(
                mailbox.clone(),
                None,
                Arc::new(CaughtUpRestaurants),
                connections,
                gateway,
            ),
            mailbox,
        )
    }

    /// The enqueued entries of one command type (attempt-scoped ids make direct lookup moot).
    fn entries_of(mailbox: &MemMailbox, command_type: &str) -> Vec<MailboxEntry> {
        mailbox.entries().into_iter().filter(|e| e.message_type() == command_type).collect()
    }

    #[tokio::test]
    async fn connect_provisions_the_derived_aggregates_and_stores_the_token() {
        let connections = Arc::new(MemHubRiseConnections::default());
        let (flow, mailbox) = flow(connections.clone(), fake_gateway("tok_1"));

        let summary = flow.connect("the-code").await.unwrap();

        assert_eq!(summary.hubrise_account_id, "acc_1");
        assert_eq!(summary.restaurant_account_id, derive_restaurant_account_id("acc_1").0);
        assert_eq!((summary.locations, summary.catalogs_created, summary.catalogs_imported), (1, 1, 1));
        assert_eq!(summary.warnings, Vec::<String>::new());

        // Fire-and-forget (ADR-20260731-122500): the connect ENQUEUES the four provisioning
        // commands — the mailbox worker delivers them and the aggregates decide. What this flow
        // owns is the hand-off: one WORKER-channel entry per command, addressed to the derived
        // lanes, payloads carrying the pulled data.
        let acc = entries_of(&mailbox, "RegisterRestaurantAccount");
        assert_eq!(acc.len(), 1);
        assert_eq!(acc[0].kind(), "COMMAND");
        assert_eq!(acc[0].channel(), "WORKER");
        assert_eq!(acc[0].actor_type(), "RestaurantAccount");
        assert_eq!(acc[0].actor_id(), derive_restaurant_account_id("acc_1").0);
        assert_eq!(acc[0].payload()["legalName"], serde_json::json!("Bella Pizza"));
        assert_eq!(acc[0].payload()["defaultCurrency"], serde_json::json!("EUR"));
        assert_eq!(acc[0].payload()["ref"], serde_json::json!("acc_1"));

        let resto = entries_of(&mailbox, "RegisterRestaurant");
        assert_eq!(resto.len(), 1);
        assert_eq!(resto[0].actor_type(), "Restaurant");
        assert_eq!(resto[0].actor_id(), derive_restaurant_id("loc_1").0);
        assert_eq!(resto[0].payload()["accountId"], serde_json::json!(derive_restaurant_account_id("acc_1").0));
        assert_eq!(resto[0].payload()["listingStatus"], serde_json::json!("PASSIVE_PARTNER"));
        assert_eq!(resto[0].payload()["ref"], serde_json::json!("loc_1"));
        assert_eq!(resto[0].payload()["address"]["city"], serde_json::json!("Tours"));
        assert_eq!(resto[0].payload()["preparationTimeMinutes"], serde_json::json!(15));

        // The catalog: created AND initially imported (no waiting for the first callback), on the
        // SAME Catalog lane — head-of-line order delivers CreateCatalog before ImportCatalog.
        let create = entries_of(&mailbox, "CreateCatalog");
        let import = entries_of(&mailbox, "ImportCatalog");
        assert_eq!((create.len(), import.len()), (1, 1));
        assert_eq!(create[0].actor_id(), derive_catalog_id("cat_1").0);
        assert_eq!(import[0].actor_id(), derive_catalog_id("cat_1").0, "same lane = ordered delivery");
        assert_eq!(import[0].payload()["products"].as_array().map(Vec::len), Some(1));

        // The token is stored keyed by the RestaurantAccount, with the location snapshot for
        // callback→token resolution.
        let conn = connections.connection(derive_restaurant_account_id("acc_1").0).unwrap();
        assert_eq!(conn.access_token, "tok_1");
        assert_eq!(conn.hubrise_account_id, "acc_1");
        let loc = connections.location("loc_1").unwrap();
        assert_eq!(loc.restaurant_id, derive_restaurant_id("loc_1").0);
    }

    #[tokio::test]
    async fn reconnect_re_enqueues_under_fresh_attempt_ids_and_refreshes_the_token() {
        let connections = Arc::new(MemHubRiseConnections::default());
        // One shared mailbox across both attempts (the door is the same in production).
        let mailbox = Arc::new(MemMailbox::instantly_delivered());
        let first = HubRiseConnectFlow::new(
            mailbox.clone(),
            None,
            Arc::new(CaughtUpRestaurants),
            connections.clone(),
            fake_gateway("tok_1"),
        );
        first.connect("the-code").await.unwrap();
        let entries_before = mailbox.entries().len();
        assert_eq!(entries_before, 4, "account + restaurant + create + import");

        // The operator re-connects the SAME HubRise account (new OAuth round-trip, new token).
        // Each attempt enqueues under ITS OWN attempt-scoped ids — replay absorption is the
        // AGGREGATES' job at delivery (their creation idempotency), not the mailbox key's.
        let second = HubRiseConnectFlow::new(
            mailbox.clone(),
            None,
            Arc::new(CaughtUpRestaurants),
            connections.clone(),
            fake_gateway("tok_2"),
        );
        let summary = second.connect("the-code").await.unwrap();

        assert_eq!(summary.warnings, Vec::<String>::new());
        assert_eq!(mailbox.entries().len(), entries_before + 4, "a fresh fan-out per attempt");
        let conn = connections.connection(derive_restaurant_account_id("acc_1").0).unwrap();
        assert_eq!(conn.access_token, "tok_2", "a re-connect refreshes the stored token");
    }

    #[tokio::test]
    async fn a_connection_without_an_account_in_scope_fails_and_stores_nothing() {
        let connections = Arc::new(MemHubRiseConnections::default());
        let mut gateway = fake_gateway("tok_1");
        gateway.token =
            serde_json::from_value(serde_json::json!({ "access_token": "tok_1" })).unwrap();
        gateway.account = serde_json::json!({ "name": "No Id", "currency": "EUR" });
        let (flow, mailbox) = flow(connections.clone(), gateway);

        let err = flow.connect("the-code").await.unwrap_err();
        assert!(matches!(err, ConnectError::NoAccountInScope), "got {err}");
        assert!(mailbox.entries().is_empty(), "nothing enqueued");
        assert!(connections.connection(derive_restaurant_account_id("acc_1").0).is_none());
    }

    #[tokio::test]
    async fn a_failed_catalog_listing_still_records_the_connection() {
        let connections = Arc::new(MemHubRiseConnections::default());
        let mut gateway = fake_gateway("tok_1");
        gateway.catalogs = Err("hubrise API returned status 500".into());
        let (flow, _mailbox) = flow(connections.clone(), gateway);

        let summary = flow.connect("the-code").await.unwrap();

        assert_eq!((summary.locations, summary.catalogs_created), (1, 0));
        assert_eq!(summary.warnings.len(), 1, "the listing failure is surfaced: {:?}", summary.warnings);
        // Account + restaurant provisioned, token stored — the enricher can serve the next callback.
        let conn = connections.connection(derive_restaurant_account_id("acc_1").0).unwrap();
        assert_eq!(conn.access_token, "tok_1");
    }
}
