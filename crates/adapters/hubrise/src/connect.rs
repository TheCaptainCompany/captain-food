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

use application::commands::{
    create_catalog, import_catalog, register_restaurant, register_restaurant_account, rejection_code,
};
use application::dispatch::{dispatch_journaled, JournaledOutcome};
use application::journal::{payload_hash, CommandJournal, CommandJournalEntry};
use application::ports::{Actor, EventStore};
use application::queries::RestaurantReadRepository;
use domain::generated::commands::{CreateCatalog, RegisterRestaurant, RegisterRestaurantAccount};
use domain::generated::entities::{Address, TaxRate};
use domain::generated::scalars::{
    AddressLine, CatalogName, CityName, CommandChannel, CommandJournalStatus, CountryCode,
    CurrencyCode, ExternalReference, PostalCode, RestaurantDisplayName, RestaurantLegalName,
    RestaurantListingStatus, Slug, TaxRatePercent, TimeZone,
};
use domain::shared::errors::DomainError;
use infrastructure::integrations::sirene::slugify;

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
    store: Arc<dyn EventStore>,
    journal: Arc<dyn CommandJournal>,
    restaurants: Arc<dyn RestaurantReadRepository>,
    connections: Arc<dyn HubRiseConnections>,
    gateway: G,
}

impl<G: HubRiseConnectGateway> HubRiseConnectFlow<G> {
    pub fn new(
        store: Arc<dyn EventStore>,
        journal: Arc<dyn CommandJournal>,
        restaurants: Arc<dyn RestaurantReadRepository>,
        connections: Arc<dyn HubRiseConnections>,
        gateway: G,
    ) -> Self {
        Self { store, journal, restaurants, connections, gateway }
    }

    /// One journaled WORKER send. `message_id` is scoped to THIS attempt (a re-connect re-sends with
    /// fresher data under new ids; the aggregates' own idempotency absorbs replays), `correlation_id`
    /// groups the attempt's whole fan-out.
    fn entry(
        attempt: uuid::Uuid,
        command_type: &str,
        entity: &str,
        payload: serde_json::Value,
    ) -> CommandJournalEntry {
        CommandJournalEntry {
            message_id: derive("connect-command", &format!("{attempt}:{command_type}:{entity}")),
            correlation_id: attempt,
            cause_id: Some(attempt),
            session_id: None,
            trace_id: None,
            user_id: Some(hubrise_system_user_id()),
            user_type: EXTERNAL_USER_TYPE,
            channel: CommandChannel::WORKER,
            command_type: command_type.to_string(),
            payload_hash: payload_hash(&payload),
            payload,
        }
    }

    fn actor(entry: &CommandJournalEntry) -> Actor {
        Actor {
            user_id: hubrise_system_user_id(),
            user_type: EXTERNAL_USER_TYPE,
            correlation_id: entry.correlation_id,
            cause_id: Some(entry.message_id),
        }
    }

    /// Dispatch one provisioning command; a deterministic rejection becomes a warning (the connect
    /// continues), an infra failure aborts. Returns `Ok(true)` when the command took effect (or was
    /// an idempotent replay), `Ok(false)` on a warned rejection.
    async fn send<F, Fut>(
        &self,
        attempt: uuid::Uuid,
        command_type: &str,
        entity: &str,
        payload: serde_json::Value,
        warnings: &mut Vec<String>,
        handler: F,
    ) -> Result<bool, ConnectError>
    where
        F: FnOnce(Actor) -> Fut,
        Fut: std::future::Future<Output = Result<(), DomainError>>,
    {
        let entry = Self::entry(attempt, command_type, entity, payload);
        let actor = Self::actor(&entry);
        let outcome = dispatch_journaled(self.journal.as_ref(), entry, move || handler(actor))
            .await
            .map_err(ConnectError::Infra)?;
        match outcome {
            JournaledOutcome::Executed(Ok(()))
            | JournaledOutcome::Deduplicated(CommandJournalStatus::SUCCEEDED) => Ok(true),
            JournaledOutcome::Executed(Err(e)) if rejection_code(&e).is_some() => {
                warnings.push(format!("{command_type} {entity}: rejected: {e}"));
                Ok(false)
            }
            JournaledOutcome::Executed(Err(e)) => Err(ConnectError::Infra(e)),
            other => {
                warnings.push(format!("{command_type} {entity}: not re-applied ({other:?})"));
                Ok(false)
            }
        }
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
        let payload = serde_json::to_value(&cmd)
            .map_err(|e| ConnectError::Infra(DomainError::Repository(e.to_string())))?;
        let store = self.store.clone();
        self.send(
            attempt,
            "RegisterRestaurantAccount",
            &hubrise_account_id,
            payload,
            &mut warnings,
            move |actor| async move {
                register_restaurant_account(store.as_ref(), cmd, &actor).await
            },
        )
        .await?;

        // 2) One Restaurant per location (the location IS the restaurant; ids reconcile with the
        //    enricher's derivation so later callbacks land on these aggregates).
        let mut connected_locations = Vec::with_capacity(locations.len());
        for loc in &locations {
            let restaurant_id = derive_restaurant_id(&loc.id);
            let name = loc.name.clone().unwrap_or_else(|| format!("Location {}", loc.id));
            // slugify(name)-slugify(location id): deterministic and unique per location (SIRENE's
            // name+NIC pattern), so two same-named locations never collide.
            let base = slugify(&name);
            let suffix = slugify(&loc.id);
            let slug = match (base.is_empty(), suffix.is_empty()) {
                (false, false) => format!("{base}-{suffix}"),
                (false, true) => base,
                (true, false) => format!("restaurant-{suffix}"),
                (true, true) => format!("restaurant-{}", restaurant_id.0.simple()),
            };
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
                slug: Slug(slug),
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
            let payload = serde_json::to_value(&cmd)
                .map_err(|e| ConnectError::Infra(DomainError::Repository(e.to_string())))?;
            let store = self.store.clone();
            let restaurants = self.restaurants.clone();
            self.send(attempt, "RegisterRestaurant", &loc.id, payload, &mut warnings, {
                move |actor| async move {
                    register_restaurant(store.as_ref(), restaurants.as_ref(), cmd, &actor).await
                }
            })
            .await?;
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

            let cmd = CreateCatalog {
                catalog_id,
                restaurant_id,
                name: CatalogName(cat.name.clone().unwrap_or_else(|| "Menu".to_string())),
                r#ref: Some(ExternalReference(cat.id.clone())),
            };
            let payload = serde_json::to_value(&cmd)
                .map_err(|e| ConnectError::Infra(DomainError::Repository(e.to_string())))?;
            let store = self.store.clone();
            let restaurants = self.restaurants.clone();
            let ok = self
                .send(attempt, "CreateCatalog", &cat.id, payload, &mut warnings, {
                    move |actor| async move {
                        create_catalog(store.as_ref(), restaurants.as_ref(), cmd, &actor).await
                    }
                })
                .await?;
            if !ok {
                continue;
            }
            created += 1;

            // Initial import — same pull + ACL mapping the callback enrichment uses.
            match self.gateway.pull_catalog(&token.access_token, &cat.id).await {
                Ok(json) => match map_catalog(&json, &cat.id, &location_id) {
                    Ok(cmd) => {
                        let payload = serde_json::to_value(&cmd).map_err(|e| {
                            ConnectError::Infra(DomainError::Repository(e.to_string()))
                        })?;
                        let store = self.store.clone();
                        let ok = self
                            .send(attempt, "ImportCatalog", &cat.id, payload, &mut warnings, {
                                move |actor| async move {
                                    import_catalog(store.as_ref(), cmd, &actor).await
                                }
                            })
                            .await?;
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
    use application::journal::mem::MemCommandJournal;
    use application::ports::version_conflict;
    use application::queries::{RestaurantFilter, RestaurantRow};
    use domain::generated::events::DomainEvent;
    use domain::generated::scalars::{
        OrderAcceptanceMode, RestaurantId as RestaurantIdScalar, RestaurantStatus,
    };

    use crate::connections::mem::MemHubRiseConnections;

    // ----- in-memory event store (mirrors the Postgres UNIQUE(stream,version) guard) -----

    #[derive(Default)]
    struct InMemoryEventStore {
        streams: std::sync::Mutex<std::collections::HashMap<String, Vec<DomainEvent>>>,
    }

    impl InMemoryEventStore {
        fn events(&self, stream: &str) -> Vec<DomainEvent> {
            self.streams.lock().unwrap().get(stream).cloned().unwrap_or_default()
        }
        fn total_events(&self) -> usize {
            self.streams.lock().unwrap().values().map(Vec::len).sum()
        }
    }

    #[async_trait::async_trait]
    impl EventStore for InMemoryEventStore {
        async fn append(
            &self,
            stream_name: &str,
            expected_version: i64,
            events: &[DomainEvent],
            _actor: &Actor,
        ) -> Result<i64, DomainError> {
            let mut streams = self.streams.lock().unwrap();
            let stream = streams.entry(stream_name.to_string()).or_default();
            if stream.len() as i64 != expected_version {
                return Err(version_conflict(stream_name, expected_version));
            }
            stream.extend_from_slice(events);
            Ok(stream.len() as i64)
        }

        async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
            let streams = self.streams.lock().unwrap();
            let events = streams.get(stream_name).cloned().unwrap_or_default();
            let version = events.len() as i64;
            Ok((events, version))
        }
    }

    // ----- fake restaurant read model: a caught-up projection (by_id resolves immediately) -----

    struct CaughtUpRestaurants;

    fn dummy_row(id: RestaurantIdScalar) -> RestaurantRow {
        RestaurantRow {
            restaurant_id: id,
            restaurant_account_id: None,
            listing_status: RestaurantListingStatus::PASSIVE_PARTNER,
            external_identifiers: None,
            google_place_id: None,
            slug: Slug("x".into()),
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
        store: Arc<InMemoryEventStore>,
        connections: Arc<MemHubRiseConnections>,
        gateway: FakeGateway,
    ) -> (HubRiseConnectFlow<FakeGateway>, Arc<MemCommandJournal>) {
        let journal = Arc::new(MemCommandJournal::default());
        (
            HubRiseConnectFlow::new(
                store,
                journal.clone(),
                Arc::new(CaughtUpRestaurants),
                connections,
                gateway,
            ),
            journal,
        )
    }

    #[tokio::test]
    async fn connect_provisions_the_derived_aggregates_and_stores_the_token() {
        let store = Arc::new(InMemoryEventStore::default());
        let connections = Arc::new(MemHubRiseConnections::default());
        let (flow, _journal) = flow(store.clone(), connections.clone(), fake_gateway("tok_1"));

        let summary = flow.connect("the-code").await.unwrap();

        assert_eq!(summary.hubrise_account_id, "acc_1");
        assert_eq!(summary.restaurant_account_id, derive_restaurant_account_id("acc_1").0);
        assert_eq!((summary.locations, summary.catalogs_created, summary.catalogs_imported), (1, 1, 1));
        assert_eq!(summary.warnings, Vec::<String>::new());

        // The account aggregate, under the ENRICHER'S derived id, seeded from the pulled account.
        let account_stream =
            format!("RestaurantAccount-{}", derive_restaurant_account_id("acc_1").0);
        let events = store.events(&account_stream);
        assert_eq!(events.len(), 1);
        let DomainEvent::RestaurantAccountRegistered(acc) = &events[0] else {
            panic!("expected RestaurantAccountRegistered, got {:?}", events[0]);
        };
        assert_eq!(acc.legal_name.0, "Bella Pizza");
        assert_eq!(acc.default_currency.0, "EUR");
        assert_eq!(acc.r#ref, Some(ExternalReference("acc_1".into())));
        assert_eq!(acc.timezone, Some(TimeZone("Europe/Paris".into())));

        // The location aggregate: derived id, owned by the account, PASSIVE_PARTNER, slugged
        // name-locationid, ref = the HubRise location id (what callbacks carry).
        let restaurant_stream = format!("Restaurant-{}", derive_restaurant_id("loc_1").0);
        let events = store.events(&restaurant_stream);
        assert_eq!(events.len(), 1);
        let DomainEvent::RestaurantRegistered(r) = &events[0] else {
            panic!("expected RestaurantRegistered, got {:?}", events[0]);
        };
        assert_eq!(r.account_id, Some(derive_restaurant_account_id("acc_1")));
        assert_eq!(r.listing_status, RestaurantListingStatus::PASSIVE_PARTNER);
        assert_eq!(r.slug.0, "bella-pizza-loc-1");
        assert_eq!(r.r#ref, Some(ExternalReference("loc_1".into())));
        assert_eq!(r.address.city.0, "Tours");
        assert_eq!(r.timezone, Some(TimeZone("Europe/Paris".into())));
        assert_eq!(r.preparation_time_minutes, Some(15));

        // The catalog: created AND initially imported (no waiting for the first callback), with the
        // id inventory callbacks will re-derive.
        let catalog_stream = format!("Catalog-{}", derive_catalog_id("cat_1").0);
        let events = store.events(&catalog_stream);
        assert!(
            matches!(&events[0], DomainEvent::CatalogCreated(c) if c.r#ref == Some(ExternalReference("cat_1".into()))),
            "first catalog event: {:?}",
            events[0]
        );
        assert!(
            matches!(&events[1], DomainEvent::CatalogImported(i) if i.products.len() == 1),
            "second catalog event: {:?}",
            events.get(1)
        );

        // The token is stored keyed by the RestaurantAccount, with the location snapshot for
        // callback→token resolution.
        let conn = connections.connection(derive_restaurant_account_id("acc_1").0).unwrap();
        assert_eq!(conn.access_token, "tok_1");
        assert_eq!(conn.hubrise_account_id, "acc_1");
        let loc = connections.location("loc_1").unwrap();
        assert_eq!(loc.restaurant_id, derive_restaurant_id("loc_1").0);
    }

    #[tokio::test]
    async fn reconnect_is_idempotent_and_refreshes_the_token() {
        let store = Arc::new(InMemoryEventStore::default());
        let connections = Arc::new(MemHubRiseConnections::default());
        let (first, _) = flow(store.clone(), connections.clone(), fake_gateway("tok_1"));
        first.connect("the-code").await.unwrap();
        let events_before = store.total_events();

        // The operator re-connects the SAME HubRise account (new OAuth round-trip, new token).
        let (second, _) = flow(store.clone(), connections.clone(), fake_gateway("tok_2"));
        let summary = second.connect("the-code").await.unwrap();

        assert_eq!(summary.warnings, Vec::<String>::new());
        // No CREATION is double-applied: the account/restaurant streams are untouched and the
        // catalog gains exactly ONE event — a fresh CatalogImported (re-import = replace semantics,
        // the legitimate effect of a re-connect).
        assert_eq!(store.total_events(), events_before + 1);
        let account_stream =
            format!("RestaurantAccount-{}", derive_restaurant_account_id("acc_1").0);
        assert_eq!(store.events(&account_stream).len(), 1);
        let restaurant_stream = format!("Restaurant-{}", derive_restaurant_id("loc_1").0);
        assert_eq!(store.events(&restaurant_stream).len(), 1);
        let catalog_events = store.events(&format!("Catalog-{}", derive_catalog_id("cat_1").0));
        assert_eq!(
            catalog_events
                .iter()
                .filter(|e| matches!(e, DomainEvent::CatalogCreated(_)))
                .count(),
            1,
            "the catalog is created once"
        );
        assert_eq!(
            catalog_events
                .iter()
                .filter(|e| matches!(e, DomainEvent::CatalogImported(_)))
                .count(),
            2,
            "each connect re-imports (replace semantics)"
        );
        let conn = connections.connection(derive_restaurant_account_id("acc_1").0).unwrap();
        assert_eq!(conn.access_token, "tok_2", "a re-connect refreshes the stored token");
    }

    #[tokio::test]
    async fn a_connection_without_an_account_in_scope_fails_and_stores_nothing() {
        let store = Arc::new(InMemoryEventStore::default());
        let connections = Arc::new(MemHubRiseConnections::default());
        let mut gateway = fake_gateway("tok_1");
        gateway.token =
            serde_json::from_value(serde_json::json!({ "access_token": "tok_1" })).unwrap();
        gateway.account = serde_json::json!({ "name": "No Id", "currency": "EUR" });
        let (flow, _) = flow(store.clone(), connections.clone(), gateway);

        let err = flow.connect("the-code").await.unwrap_err();
        assert!(matches!(err, ConnectError::NoAccountInScope), "got {err}");
        assert_eq!(store.total_events(), 0);
        assert!(connections.connection(derive_restaurant_account_id("acc_1").0).is_none());
    }

    #[tokio::test]
    async fn a_failed_catalog_listing_still_records_the_connection() {
        let store = Arc::new(InMemoryEventStore::default());
        let connections = Arc::new(MemHubRiseConnections::default());
        let mut gateway = fake_gateway("tok_1");
        gateway.catalogs = Err("hubrise API returned status 500".into());
        let (flow, _) = flow(store.clone(), connections.clone(), gateway);

        let summary = flow.connect("the-code").await.unwrap();

        assert_eq!((summary.locations, summary.catalogs_created), (1, 0));
        assert_eq!(summary.warnings.len(), 1, "the listing failure is surfaced: {:?}", summary.warnings);
        // Account + restaurant provisioned, token stored — the enricher can serve the next callback.
        let conn = connections.connection(derive_restaurant_account_id("acc_1").0).unwrap();
        assert_eq!(conn.access_token, "tok_1");
    }
}
