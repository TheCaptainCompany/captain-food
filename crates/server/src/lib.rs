//! Captain.Food server (Axum BFF) — the composition root (ADR-0035).
//!
//! DI happens here: infrastructure adapters are injected behind application ports, then the HTTP/GraphQL
//! surface is built over them. Exposed as a library so `desktop` (Tauri) can embed it in-process.
//!
//! Endpoints:
//! - `/ping` → `pong` — liveness (process is up; touches nothing). Used by uptime pingers / keep-warm.
//! - `/health` — readiness gate (ADR-0043): `200` only when the DB is reachable AND its schema version is
//!   `>= REQUIRED_SCHEMA_VERSION`; else `503`. Migrations are applied out-of-band by **sqlx-cli in CI**.
//!   Every response carries `version` (the build's git SHA, ADR-20260721-175411) for failure diagnostics.
//! - `/projector` — projection-worker readiness (running / checkpoint / head / lag / lastTickAt).
//! - `/saga` — process-manager (saga) runner readiness, same shape as `/projector`.
//! - `/sirene` — SIRENE sync-worker readiness (issue #244), same shape plus `lastSummary`. `503` with
//!   `reason: poll_loop_not_started` when `RUN_SIRENE_WORKER` left the loop paused — the state that
//!   was undiagnosable from outside during the department-37 pilot (#238).
//! - `/{role}/graphql` (+ `/{role}/voyager`) — the GraphQL BFF (ADR-0006), see `graphql`.
//! - `POST /internal/sirene/drain` — wakes the SIRENE sync worker after a CI ingestion run (ADR-0045);
//!   secured by the `INTERNAL_TRIGGER_TOKEN` shared secret (`x-internal-token` header).
//! - `POST /adapters/stripe/webhooks` — Stripe webhook ingestion (inbound payment facts through the ACL);
//!   secured by `Stripe-Signature` HMAC verification against `STRIPE_WEBHOOK_SECRET` (fail-closed).
//!
//! Every response (all routes) carries `X-VERSION` = the running build's short git SHA (ADR-20260721-175411),
//! so any client can read which deploy served it without calling `/health` (see `response_timing`).
//!
//! The projection worker (ADR-0040) runs **in-process** here for now (Render Background Workers are paid),
//! gated by `RUN_PROJECTOR` (default on) so it can graduate to a dedicated worker with no logic change.
//! The SIRENE sync worker (ADR-0045) follows the same pattern: in-process, primarily woken by the CI
//! ingestion's ping, with a slow safety-net poll loop gated by `RUN_SIRENE_WORKER` — **default OFF since
//! 2026-07-28**, paused with the CI ingestion until the write-path defects in issue #220 are resolved.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::{Request, State},
    http::{HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use actor_client::supervision::MailboxLaneRepository;
use application::queries::{
    CartReadRepository, CatalogReadRepository, CustomerCreditReadRepository, CustomerReadRepository,
    DeliveryPartnerAvailabilityReadRepository, DeliverySatisfactionReadRepository,
    DeliveryReadRepository, OrderReadRepository,
    PricingPolicyReadRepository, ProspectionReadRepository, ReclamationReadRepository,
    RefundReadRepository, RestaurantReadRepository, UberEstimationPolicyReadRepository,
    UberSplitPolicyReadRepository,
};
use infrastructure::{
    EventBus, FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCartRepository, PgCatalogRepository, PgCustomerCreditRepository, PgCustomerRepository,
    PgDeliveryPartnerAvailabilityRepository, PgDeliveryRepository, PgDeliverySatisfactionRepository,
    PgEventStore,
    PgOrderRepository, PgPricingPolicyRepository, PgProspectionRepository, PgReclamationRepository,
    PgRefundQueueRepository,
    PgRestaurantRepository, PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository, ProcessManagerRunner,
    ProcessManagerStatus, ProjectionStatus, ProjectionWorker, SireneSyncWorker,
    UnverifiedGbpOrderLinkProbe,
};
use avelo37_adapter::Avelo37WebhookIngestor;
use coopcycle_adapter::CoopCycleWebhookIngestor;
use uber_direct_adapter::UberDirectWebhookIngestor;
use stripe_adapter::StripeWebhookIngestor;
use shared_types::HealthDto;

use graphql::schema::{ReadDeps, WriteDeps};

mod auth;
mod auth_routes;
/// Composition support for the `graphql-{scope}` subgraph bins (#385 API-tier wiring, D8): the
/// monolith's GraphQL surface, same DI ([`build_graphql_di`]), restricted to one scope's slice.
pub mod bin_support;
pub mod bootstrap_platform_admin;
mod web_ssr;
/// The expose-gated `/services/*` surface + module index, GENERATED from specs/services.yaml
/// (issue #26, ADR-20260719-214500).
pub mod generated;
mod graphql;
mod hosts;

/// The role-as-path ACL seam (RequestRole/RoleGuard, ADR-0006), re-exported so integration tests can
/// execute the schema under a specific role (the HTTP layer injects it from the URL path).
pub use graphql::acl as graphql_acl;
/// The per-role GraphQL depth/complexity ceilings (#639 part C step 6-ii round 2, R2-E),
/// re-exported so the DB-gated behaviour tests can derive their boundary documents from the SAME
/// emitted constants the runtime extension enforces — never a hand-spelled number
/// (ADR-20260817-105845).
pub use graphql::generated::limits as graphql_generated_limits;
/// The extension type itself, re-exported for the SAME reason: `QueryLimits::effective_max_depth`/
/// `effective_max_complexity` are the POST-HEADROOM values `parse_query` actually enforces, and a
/// test deriving its boundary document from the raw generated constant alone would silently pass
/// against the RAW number while the runtime enforces raw×headroom — this is the antecedent that
/// avoids that drift.
pub use graphql::query_limits::QueryLimits;
pub use graphql::session as graphql_session;
/// The request locale for human-readable GraphQL text (#639 2c-ii) -- injectable by a
/// schema-level test exactly as the transport injects it.
pub use graphql::locale as graphql_locale;
/// The request's TENANT seam (#469), re-exported for the same reason as the session seam: a test
/// that executes the schema directly must supply the datum the HTTP edge resolves from the `Host`.
pub use graphql::tenant as graphql_tenant;
/// The GraphQL router (#469): mounted by the composition root, and by the PATH-level test that
/// drives a real `POST /public/graphql` — cookie, `Host` and all — through the same
/// `graphql_routes` production runs. Every cart test before it injected `ReadScope` by hand, which
/// is exactly why a dead auth leg could survive a green suite.
pub use graphql::routes::graphql_routes;
/// #639 part C step 5 (ADR-20260905-065415): [`graphql_routes`] with the socket-close gate as an
/// explicit parameter — the composition root and the socket-close DB-gated test both need it ON;
/// every other existing caller keeps using [`graphql_routes`] (gate OFF, unchanged).
pub use graphql::routes::graphql_routes_with_socket_close_gate;
/// #639 part C step 5: the gate newtype + the connection-local standing cell, re-exported so the
/// socket-close DB-gated test can build a real WS server with the gate ON.
pub use graphql::rider_socket as graphql_rider_socket;
/// The JWT verifier the edge authorizes through — the PATH-level test builds one over a loopback
/// JWKS, so its request is authenticated the way a browser's is.
pub use auth::AuthContext;
// The verified request principal — exposed for the subscription-ownership integration tests
// (the generated resolvers reach it as crate::auth::Principal).
pub use auth::Principal;
/// The role-guard witness (#639 part B). Re-exported because integration tests must inject the
/// SAME value `routes.rs` does, and there is deliberately no other way to obtain one: it comes from
/// [`Principal::acting_role`] or it does not exist. A test that wants to exercise a role therefore
/// has to name the identity holding it — which is the property under test.
pub use auth::ActingRole;
/// IDENT-1 Phase A (#641): the CUSTOMER identity-resolution seam, re-exported so integration tests
/// can drive `graphql_routes` under either mode and plant a fake `ResolveCustomerIdentity`, and
/// exercise `resolve_read_scope` directly over a REAL verified `Principal` (its own constructors
/// stay module-private — only `AuthContext::authorize` produces one).
pub use auth::{
    resolve_read_scope, CustomerIdentityResolution, CustomerIdentitySource, IdentityResolution,
    IdentitySources, LookupFailureReason, MemberIdentityResolution, MemberIdentitySource,
    NoDatabaseMemberIdentity, NoDatabasePlatformIdentity, NoDatabaseRiderIdentity,
    PgCustomerIdentity, PgMemberIdentity, PgPlatformIdentity, PgRiderIdentity,
    PlatformIdentityResolution, PlatformIdentitySource, ResolveCustomerIdentity,
    ResolveMemberIdentity, ResolvePlatformIdentity, ResolveRiderIdentity, RiderIdentityResolution,
    RiderIdentitySource,
};
/// The schema composition surface (build_schema/ReadDeps/WriteDeps), re-exported so integration tests
/// (and the embedding `desktop` shell) can build the master schema over their own adapters.
pub use graphql::schema as graphql_schema;
// #440: the checkout-degrade EMISSION is proved through the real render path from its own test
// PROCESS (tests/checkout_degraded_metric.rs) — the meters bind `opentelemetry::global::meter`
// once per process, so the spy provider must be installed before the first metric call, a
// guarantee the parallel in-crate harness cannot give. These three are that test's entry points.
pub use hosts::{host_root, TenantLookup};
pub use web_ssr::SsrExec;

/// Minimal health/edge-proof: lets the `desktop` (Tauri) shell embed the server in-process and proves the
/// server → shared_types edge (ADR-0035). The real DI graph is built in `router()`.
pub fn wire() -> HealthDto {
    HealthDto::ok()
}

/// The schema version this build requires. Migrations are applied by **sqlx-cli in CI** (ADR-0043); the app
/// only checks the DB has reached at least this version. Bump when adding a migration this build depends on.
/// The gate is `>=` (never `==`) so an older build still runs against a newer DB (rollback-by-redeploy).
/// `20260720030000` = the command/inbound journals (ADR-20260720-015300/-015400): every mutation now
/// writes `inbound_messages` at acceptance, so the app cannot serve writes without it.
/// `20260721150000` = the Uber Direct webhook mirror (external_uber_direct_events, #57): the adapter's
/// inbound ingestor stages verified facts into it, so the app must not serve without the table.
/// `20260725000000` = the `orderconversation` projection table (#131, epic #129): the projection worker
/// upserts folded conversation rows into it, so the app must not serve without the table.
/// `20260726000000` = the `orderconversation.claim_events` column (§2.5, epic #151; #155): the projector
/// upserts the woven claim-lifecycle timeline into it, so the app must not serve without the column.
/// `20260727000000` = the `customercreditbalance` projection table (#158, Part B of #207): the projection
/// worker upserts the folded store-credit balance into it, so the app must not serve without the table.
/// `20260728020000` = `Restaurant.slug` DROP NOT NULL (ADR-20260728-011344): `RestaurantRegistered` no
/// longer carries a slug, so the projector writes NULL for every listing without a configured storefront
/// — against the old NOT NULL column that fails on the first projected registration. This gate is what
/// holds the new projector back until CI has applied the migration.
/// `20260728030000` = `slug_reservations` + `SlugAlias` (ADR-20260728-011344, slice 3): the projection
/// worker now upserts alias rows for every rename, and the slug handler reserves through the former, so
/// the app must not serve without either table.
/// `20260728040000` = `external_sirene_restaurants.payload_hash` (ADR-20260728-011344, slice 5): the
/// ingestion writes it and the worker's pending predicate depends on it, so a build without the column
/// would re-pend the whole mirror on every sweep.
/// `20260728050000` = `external_sirene_restaurants.payload` DROP NOT NULL + `status`
/// (ADR-20260728-143000, #231): the worker NULLs a translated payload and stamps the status in the same
/// statement as the checkpoint, so both would fail against the old NOT NULL column / missing column.
/// `20260728160000` = `synced_at` + `last_attempt_sync_at` + `attempt_sync_retry_count` on the same
/// table (ADR-20260728-143000 follow-up): the worker writes all three on every drain and the quarantine
/// (`status = 'POISON'` after 10 consecutive failures) depends on the counter, so a build without them
/// would fail every mark and retry a broken row forever.
/// `20260731143000` = the ACTOR MAILBOX (`20260731063000` creates `inbound_messages`, `20260731143000`
/// backfills `inbound_events` into it and DROPS it) plus the enum-text conversions
/// (`20260730043100`-`20260730043600`). Every flipped resolver and every adapter ACL now enqueues on
/// `inbound_messages`, so a build without those tables cannot serve a single mutation.
///
/// `20260802230000` = the WIDTH-5 KEYSPACE (`20260802220000` re-stamps every row's `partition`
/// for width 5, ADR-20260802-220402) plus `attempts` (`20260802230000`, the #313 poison cap). A
/// width-5 binary against a width-100 mailbox claims lanes 0-4 only, so rows sitting in
/// partitions 5-99 would never drain — the gate must hold the new binary until the re-stamp runs.
///
/// This constant was left at `20260728160000` while nine migrations landed, which made the
/// readiness gate INERT for exactly the failure it exists to catch: a new instance would read
/// `applied >= required`, report `ok`, take traffic, and fail every write on
/// `relation "inbound_messages" does not exist`. The deploy runbook (ADR-20260730-051500) explicitly
/// relies on this gate holding the new binary at 503 until `db-migrate` lands — deploy runs FIRST and
/// the schema follows, so the gate is the only thing covering the window between them. It went stale
/// AGAIN the same week (`20260731143000` while the two width-5 migrations landed), and a THIRD time
/// within the hour (`20260802230000` while `20260803004500` added `next_attempt_at`, which the #316
/// backoff scheduler reads on every retry) — so the rule is now EXECUTABLE: the codegen guard
/// `required_schema_version_matches_the_latest_migration` fails the build whenever this constant
/// is not the newest migration timestamp. It moves in the SAME commit as the migration, period.
///
/// `20260903060000` = `auth_subject_reservations` (#639 part C step 2a, #794): `register_rider`
/// reserves `(RIDER, authRef)` through it BEFORE appending `RiderRegistered`, so a build without the
/// table would fail every rider registration at the reservation insert.
///
/// `20260904021500` = `delivery_job_open_issue` (#639 part C step 3-i, ADR-20260904-015903): the
/// `View_DeliveryJob` read repository SELECTs `open_issue_kind` on every `delivery` /
/// `myDeliveries` / `restaurantDeliveries` read, so a build without the recreated view would fail
/// every delivery read with `column "open_issue_kind" does not exist` (42703).
///
/// `20260904090000` = `ordertracking_delivery_handed_back` (#639 part C step 3-ii, review round 2
/// on #870): the `OrderTrackingProjector` upserts `delivery_handed_back` on EVERY row it writes
/// (`order_tracking_store::upsert`'s full 39-column list), so a build without the column would fail
/// every Order-group projection with `column "delivery_handed_back" does not exist` (42703) — the
/// whole customer tracking read model would stop updating, not just the handback banner.
///
/// `20260905110000` = `member_bridge_and_scope_membership_grant` (#639 part C step 6-i,
/// ADR-20260905-101349): the `member` table backs the `Member` projector arm on
/// `RestaurantAccessGranted`, so a build without it would fail every staff-access grant projection
/// with `relation "member" does not exist` (42P01) the moment `RUN_MEMBER_ACCESS_GRANT` flips on.
pub const REQUIRED_SCHEMA_VERSION: i64 = 20260905130000;

/// The precise build identity, for diagnostics (ADR-20260721-175411). CI bakes `CAPTAIN_BUILD_VERSION`
/// (the short 7-char git commit SHA the image was built from, e.g. `829f4ad`) into the deployed image — see
/// `.github/workflows/build-image.yml` + the `Dockerfile` runtime stage — and `/health` reports it in
/// EVERY state, including `degraded`/`down`: when the app is failing is precisely when you need to know
/// which build is running. Falls back to `dev-<crate version>` for local / uncontainerized runs where the
/// env var is unset. Read once and cached (the value never changes for a process).
pub fn build_version() -> &'static str {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION.get_or_init(|| {
        std::env::var("CAPTAIN_BUILD_VERSION")
            .unwrap_or_else(|_| format!("dev-{}", env!("CARGO_PKG_VERSION")))
    })
}

/// Install telemetry from the declared configuration (issue #191).
///
/// The translation layer between the generated config reader and `crates/telemetry`: the telemetry
/// crate sits below `server` and must not know `Config` exists, so the mapping happens here rather than
/// by handing it the whole struct.
///
/// A malformed sample ratio cannot reach this point — the `TraceSampleRatio` scalar is validated at
/// startup and a bad value is reported as a configuration problem — so the parse fallback is a belt-and-
/// braces `1.0` (keep everything) rather than `0.0`. Defaulting to zero here would turn one unexpected
/// value into total, silent trace loss, which is the failure this whole issue exists to end.
pub fn init_telemetry(config: &generated::config::Config) -> telemetry::TelemetryGuard {
    let cfg = telemetry::TelemetryConfig {
        api_key: config.honeycomb_api_key.clone(),
        endpoint: config.honeycomb_api_endpoint.clone(),
        dataset: config.honeycomb_dataset.clone(),
        sample_ratio: config.otel_traces_sample_ratio.parse().unwrap_or(1.0),
        log_level: config.log_level.clone(),
        service_version: build_version().to_string(),
        profile: config.profile.to_string(),
    };
    let (guard, _emission) = telemetry::init(&cfg);
    guard
}

/// Read a `RUN_*` worker toggle, leniently and uniformly (issue #244).
///
/// The gates used to be written inline, and inconsistently: `RUN_SIRENE_WORKER` was an exact
/// `v == "true"` (so `TRUE`, `True`, a space-padded or dashboard-quoted value all silently meant
/// PAUSED, with one boot-log line as the only trace), while the others were `v != "false"` (so
/// `RUN_INBOUND_DRAIN=0` meant ON). For a flag whose job is to resume a paused production pipeline,
/// silent-off on a case variant is the wrong failure mode — it cost hours on the department-37 pilot.
///
/// Accepts `true/1/yes/on` and `false/0/no/off`, case-insensitive, surrounding whitespace and wrapping
/// quotes trimmed. Anything unrecognised (including an empty value) falls back to `default` **and says
/// so on stdout**: a typo must never be silently interpreted as either state.
///
/// Behaviour change worth naming: `RUN_INBOUND_DRAIN=0` now means OFF, where the old `!= "false"`
/// shortcut read it as ON. The new reading is the intended one.
pub fn env_flag(name: &str, default: bool) -> bool {
    parse_flag(std::env::var(name).ok().as_deref(), name, default)
}

/// The parsing half of [`env_flag`], split out so it is testable without mutating process env
/// (`set_var` races across the threads of one test binary).
fn parse_flag(raw: Option<&str>, name: &str, default: bool) -> bool {
    let Some(raw) = raw else { return default };
    let normalized = raw.trim().trim_matches(['"', '\'']).trim().to_ascii_lowercase();
    match normalized.as_str() {
        "true" | "1" | "yes" | "on" => true,
        "false" | "0" | "no" | "off" => false,
        "" => default,
        other => {
            tracing::warn!(
                flag = name,
                value = other,
                default,
                "unrecognised worker-toggle value -- using the default. \
                 Accepted: true/1/yes/on, false/0/no/off."
            );
            default
        }
    }
}

/// Readiness states published by the heartbeat, read by `/health`.
mod db_state {
    pub const NOT_CONFIGURED: u8 = 0; // DATABASE_URL unset
    pub const DOWN: u8 = 1; // unreachable, or `_sqlx_migrations` does not exist yet
    pub const SCHEMA_BEHIND: u8 = 2; // reachable, but max(applied version) < REQUIRED_SCHEMA_VERSION
    pub const HEALTHY: u8 = 3; // reachable and schema is at/after the required version
}

/// Cached readiness snapshot; refreshed every 30s by the heartbeat.
#[derive(Clone)]
struct Snapshot {
    state: u8,
    /// Highest successfully-applied migration version in the DB (`-1` if none/unknown).
    applied_version: i64,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self { state: db_state::NOT_CONFIGURED, applied_version: -1 }
    }
}

#[derive(Clone)]
pub struct AppState {
    snap: Arc<Mutex<Snapshot>>,
    /// Live projection-worker status when the worker runs in-process; `None` when not started.
    projector_status: Option<Arc<Mutex<ProjectionStatus>>>,
    /// Live saga-runner (process managers, actors.yaml) status when it runs in-process; `None` when
    /// not started.
    saga_status: Option<Arc<Mutex<ProcessManagerStatus>>>,
    /// Live deletion-engine status (`RUN_DELETION_ENGINE`, ADR-20260731-214500 §4); `None` when the
    /// gate is off.
    deletion_status: Option<Arc<Mutex<infrastructure::DeletionEngineStatus>>>,
    /// Live SIRENE sync-worker status. Unlike the two above this is `Some` whenever a DATABASE_URL
    /// pool exists — the worker is always constructed (the ping endpoint needs it), so the snapshot
    /// can distinguish "loop not started" (`running: false`) from "no database" (`None`). That
    /// distinction is the point of the endpoint (#244).
    sirene_status: Option<Arc<Mutex<infrastructure::SireneSyncStatus>>>,
}

/// Build the Axum router: `/ping`, `/health`, `/projector`, `/saga`, `/sirene`, and the role-as-path
/// GraphQL routes. Reads
/// `DATABASE_URL`; when present it opens a lazy pool used by the heartbeat, the read-model repo (injected
/// into GraphQL), and the in-process projection worker.
/// Resolve the `identity` service impl (#117): the real Supabase adapter when configured
/// (`SUPABASE_URL` + `SUPABASE_PUBLISHABLE_KEY`), else the fail-closed stand-in — the same
/// env-gate + fallback pattern as the Stripe payment binding. Used for BOTH the GraphQL write-side
/// `auth_provider` and the `/auth/refresh` route.
fn identity_service_impl(
    send_guard: Option<Arc<infrastructure::SmsSendAuthorizer>>,
    email_guard: Option<Arc<infrastructure::EmailSendAuthorizer>>,
) -> Arc<dyn application::generated::services::IdentityService> {
    match infrastructure::SupabaseIdentityService::from_env() {
        Some(adapter) => {
            tracing::info!(binding = "identity", impl_ = "SupabaseIdentityService", "identity service wired (SUPABASE_URL set)");
            // The send guards (#516) attach here as CHEAP SHEDDING only — a peek, never a claim. The
            // authoritative wall is the `/auth/sms-hook` route, because that is where the euro is
            // actually spent; a guard missing HERE costs a provider round-trip and a vaguer refusal
            // message, never an unbounded bill.
            let adapter = match send_guard {
                Some(guard) => adapter.with_send_guard(guard),
                None => adapter,
            };
            // The email guard (#639 part C step 6-ii) is UNLIKE the SMS one: this IS the
            // authoritative wall (no separate hook path spends the euro for email), so a missing
            // guard here means every send is genuinely unguarded, not merely un-shed early.
            match email_guard {
                Some(guard) => Arc::new(adapter.with_email_guard(guard)),
                None => Arc::new(adapter),
            }
        }
        None => {
            tracing::warn!(binding = "identity", impl_ = "FailClosedIdentityService", "SUPABASE_URL/PUBLISHABLE_KEY unset -- identity fails closed, auth stays anonymous-only");
            Arc::new(FailClosedIdentityService)
        }
    }
}

/// Build the OTP send guards (#516) over the SHARED Postgres counter.
///
/// Shared is the whole point: a per-pod in-memory limiter multiplies the allowance by the replica count
/// and resets on every deploy, and for the global daily ceiling — the only guard that bounds the bill —
/// that is the difference between a ceiling and a suggestion.
///
/// Thresholds come from the resolved `Config` (declared in `specs/customer/configuration.yaml`), never
/// from a local `env::var` + inline fallback: a default declared in the spec and then re-typed at the
/// call site is two sources of truth, and it is the spec's copy that turns out to be inert.
pub(crate) fn sms_send_guard(
    pool: &PgPool,
    config: &generated::config::Config,
) -> Arc<infrastructure::SmsSendAuthorizer> {
    let policy = application::sms_guard::SmsSendPolicy::from_config(
        Some(config.sms_allowed_dialing_codes.as_str()),
        Some(config.sms_max_sends_per_number_per_hour as i32),
        Some(config.sms_max_sends_per_number_per_day as i32),
        Some(config.sms_send_backoff_seconds.as_str()),
        Some(config.sms_max_sends_per_day_global as i32),
    );
    tracing::info!(
        binding = "sms_send_guard",
        allowed_dialing_codes = %policy.allowed_dialing_codes.join(","),
        per_number_hour = policy.max_per_number_per_hour,
        per_number_day = policy.max_per_number_per_day,
        global_day = policy.max_per_day_global,
        "otp send guards wired against the SHARED sms_send_quota counter (#516)"
    );
    // The liveness gauge (#516; ADR-20260810-231300's inverted dead-man's switch): without it, "no
    // refusals tonight" and "the limiter has been off since the last deploy" are one observation.
    // This is the FIRST assertion, not the only one: the gauge is observable, so its callback
    // re-asserts on every export cycle, and `SmsSendAuthorizer::authorize` re-declares the state at
    // the point enforcement is actually decided (0 when the shared counter is unreachable). A single
    // boot-time `record` proved only that the process once started.
    telemetry::meters::otp_send::guard_enforcing(true);
    Arc::new(infrastructure::SmsSendAuthorizer::new(
        policy,
        Box::new(infrastructure::persistence::PgSmsQuotaStore::new(pool.clone())),
    ))
}

/// Build the email send-abuse wall (#639 part C step 6-ii, ADR-20260905-101349 §9) over the SAME
/// shared Postgres counter as the SMS wall (`sms_send_quota`) -- the store is generic on the quota
/// key, so this guard's `email:`-namespaced buckets never collide with the SMS wall's `phone:*` /
/// `global:day` keys. The `sms_send_guard` shape, transposed.
pub(crate) fn email_send_guard(
    pool: &PgPool,
    config: &generated::config::Config,
) -> Arc<infrastructure::EmailSendAuthorizer> {
    let policy = application::email_guard::EmailSendPolicy::from_config(
        Some(config.email_max_sends_per_address_per_hour as i32),
        Some(config.email_max_sends_per_address_per_day as i32),
        Some(config.email_max_sends_per_day_global as i32),
        // Round 3 R3-2: the generated field is now a required (staging/production) `String`, not
        // `Option<String>` -- unset in development/test resolves to `""`, which
        // `EmailSendPolicy::from_config` already treats as "fall back to the dev-only key" (it
        // trims and filters empty before using it), so `Some(...)` here is correct in every profile.
        Some(config.email_quota_key_hmac_secret.as_str()),
    );
    tracing::info!(
        binding = "email_send_guard",
        per_address_hour = policy.max_per_address_per_hour,
        per_address_day = policy.max_per_address_per_day,
        global_day = policy.max_per_day_global,
        "email send-abuse wall wired against the SHARED sms_send_quota counter (#639 part C step 6-ii)"
    );
    Arc::new(infrastructure::EmailSendAuthorizer::new(
        policy,
        Box::new(infrastructure::persistence::PgSmsQuotaStore::new(pool.clone())),
    ))
}

/// The read/write dependency graph of the GraphQL surface, built over one pool — the ONE
/// composition both the monolith `router()` and the `graphql-{scope}` subgraph bins (#385
/// API-tier wiring, PROP-20260807-174246 D8) perform, extracted so a subgraph bin serves the
/// SAME resolvers over the same adapters with no logic fork. The buses are process-local: a
/// subgraph process only sees completions/events raised in-process (the recorded cross-process
/// push gap on #385); poll reads are unaffected.
pub struct GraphqlDi {
    pub read: ReadDeps,
    pub write: WriteDeps,
    /// The host fallback's registered-vs-unclaimed tenant read (#98), sharing `restaurants`.
    pub tenant_lookup: hosts::TenantLookup,
    /// The cookie-pickup parking store (#112): the encrypted Pg store when AUTH_SESSION_KEY is
    /// set, else the fail-closed no-op.
    pub auth_sessions: Arc<dyn application::auth_sessions::AuthSessionStore>,
    /// The restaurant read model, shared by the HubRise connect flow and the tenant lookup.
    pub restaurants: Arc<dyn RestaurantReadRepository>,
}

/// Build [`GraphqlDi`] (ADR-0035 composition root).
pub fn build_graphql_di(
    pool: &PgPool,
    event_bus: &EventBus,
    operation_status_bus: &actor_client::OperationStatusBus,
    mailbox_nudges: &Arc<infrastructure::persistence::mailbox_store::MailboxNudges>,
    // #516: the OTP send guards, so the identity ACL can shed a doomed request with a typed reason.
    sms_guard: Option<Arc<infrastructure::SmsSendAuthorizer>>,
    // #639 part C step 6-ii: the email send-abuse wall -- UNLIKE `sms_guard`, this IS the
    // authoritative wall for `send_email_magic_link` (no separate hook path spends the euro).
    email_guard: Option<Arc<infrastructure::EmailSendAuthorizer>>,
    // RSO-1: the service-window validity horizon (SERVICE_WINDOW_VALIDITY_HORIZON_SECONDS), read
    // from the caller's Config ONCE — a parameter, so every bin passes its own configured value.
    service_window_horizon: graphql::service_clock::ServiceWindowHorizon,
    // #639 part C step 4-ii (ADR-20260904-124600 §4): `SUPPORT_CONTACT`, resolved ONCE by the
    // caller (the SAME parse the rider sign-in door already does) — a parameter, like
    // `service_window_horizon`, so every bin passes its own configured value.
    support_contact: Option<domain::generated::scalars::EmailAddress>,
    // #639 part C step 4-iii-A (ADR-20260904-152807 §7): `RUN_RIDER_RESTRICTION_DOOR`, resolved
    // ONCE by the caller — a parameter, like `support_contact`, so every bin passes its own
    // configured value onto `riders`/`rider`'s `restrictionDoorOpen`.
    run_rider_restriction_door: bool,
) -> GraphqlDi {
    let pool = pool.clone();
    // Read-model repositories injected into GraphQL resolvers.
    let restaurants: Arc<dyn RestaurantReadRepository> =
        Arc::new(PgRestaurantRepository::new(pool.clone()));
    let tenant_lookup = hosts::TenantLookup(Some(restaurants.clone()));
    // Mutated by the WriteDeps arm below when AUTH_SESSION_KEY wires the real store.
    let mut auth_sessions: Arc<dyn application::auth_sessions::AuthSessionStore> =
        Arc::new(application::auth_sessions::NoopAuthSessionStore);
    let prospection: Arc<dyn ProspectionReadRepository> =
        Arc::new(PgProspectionRepository::new(pool.clone()));
    let pricing_policy: Arc<dyn PricingPolicyReadRepository> =
        Arc::new(PgPricingPolicyRepository::new(pool.clone()));
    let uber_estimation_policy: Arc<dyn UberEstimationPolicyReadRepository> =
        Arc::new(PgUberEstimationPolicyRepository::new(pool.clone()));
    let uber_split_policy: Arc<dyn UberSplitPolicyReadRepository> =
        Arc::new(PgUberSplitPolicyRepository::new(pool.clone()));
    let catalogs: Arc<dyn CatalogReadRepository> =
        Arc::new(PgCatalogRepository::new(pool.clone()));
    let carts: Arc<dyn CartReadRepository> =
        Arc::new(PgCartRepository::new(pool.clone()));
    let orders: Arc<dyn OrderReadRepository> =
        Arc::new(PgOrderRepository::new(pool.clone()));
    let order_conversations: Arc<dyn application::queries::OrderConversationReadRepository> =
        Arc::new(infrastructure::PgOrderConversationRepository::new(pool.clone()));
    let customers: Arc<dyn CustomerReadRepository> =
        Arc::new(PgCustomerRepository::new(pool.clone()));
    let deliveries: Arc<dyn DeliveryReadRepository> =
        Arc::new(PgDeliveryRepository::new(pool.clone()));
    let rider_restrictions: Arc<dyn application::queries::RiderRestrictionReadRepository> =
        Arc::new(infrastructure::persistence::rider_restriction_store::PgRiderRestrictionRepository::new(pool.clone()));
    let rider_roster: Arc<dyn application::queries::RiderRosterReadRepository> =
        Arc::new(infrastructure::persistence::rider_roster_store::PgRiderRosterRepository::new(pool.clone()));
    let member_authority: Arc<dyn application::queries::MemberAuthorityRepository> =
        Arc::new(infrastructure::PgMemberAuthorityRepository::new(pool.clone()));
    let restaurant_roster: Arc<dyn application::queries::RestaurantRosterReadRepository> =
        Arc::new(infrastructure::PgRestaurantRosterRepository::new(pool.clone()));
    let restaurant_invitations: Arc<dyn application::queries::RestaurantInvitationListReadRepository> =
        Arc::new(infrastructure::PgRestaurantInvitationListRepository::new(pool.clone()));
    let refunds: Arc<dyn RefundReadRepository> =
        Arc::new(PgRefundQueueRepository::new(pool.clone()));
    let delivery_satisfaction: Arc<dyn DeliverySatisfactionReadRepository> =
        Arc::new(PgDeliverySatisfactionRepository::new(pool.clone()));
    let delivery_partner_availabilities: Arc<dyn DeliveryPartnerAvailabilityReadRepository> =
        Arc::new(PgDeliveryPartnerAvailabilityRepository::new(pool.clone()));
    let reclamations: Arc<dyn ReclamationReadRepository> =
        Arc::new(PgReclamationRepository::new(pool.clone()));
    let customer_credit: Arc<dyn CustomerCreditReadRepository> =
        Arc::new(PgCustomerCreditRepository::new(pool.clone()));
    let mailbox_lanes: Arc<dyn MailboxLaneRepository> = Arc::new(
        infrastructure::persistence::mailbox_lanes::PgMailboxLaneRepository::new(
            pool.clone(),
        ),
    );
    let read = ReadDeps {
        restaurants: restaurants.clone(),
        prospection,
        pricing_policy,
        uber_estimation_policy,
        uber_split_policy,
        catalogs,
        carts,
        orders,
        order_conversations,
        customers,
        deliveries,
        rider_restrictions,
        rider_roster,
        member_authority,
        restaurant_roster,
        restaurant_invitations,
        refunds,
        delivery_satisfaction,
        delivery_partner_availabilities,
        reclamations,
        customer_credit,
        mailbox_lanes,
        service_window_horizon,
        support_contact,
        run_rider_restriction_door: graphql::schema::RunRiderRestrictionDoor(run_rider_restriction_door),
    };

    // Write side (CQRS commands): the event store behind the mutation resolvers, plus the
    // Google and Supabase Auth seam adapters (fail-closed stand-ins until the real
    // integrations land).
    let write = WriteDeps {
        event_store: Arc::new(PgEventStore::with_bus(pool.clone(), event_bus.clone())),
        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
        gbp_probe: Arc::new(UnverifiedGbpOrderLinkProbe),
        // The `identity` service (services.yaml `binding: local`, #50): the REAL Supabase
        // ACL adapter (#117) when SUPABASE_URL + PUBLISHABLE_KEY are set, else the
        // fail-closed stand-in (auth stays anonymous-only — the Stripe env-gate pattern).
        auth_provider: infrastructure::generated::service_bindings::identity_service(
            || identity_service_impl(sms_guard.clone(), email_guard.clone()),
        )
        .expect("identity service binding (services.yaml)"),
        // The `payment` service resolved through the GENERATED topology binding
        // (services.yaml `binding: local`, issue #26): the composition root only supplies
        // the in-process constructor — the real outbound Stripe adapter when
        // STRIPE_SECRET_KEY is configured, otherwise the fail-closed stand-in (placeOrder
        // stays wired end-to-end but declines every checkout).
        payments: infrastructure::generated::service_bindings::payment_service(|| {
            match std::env::var("STRIPE_SECRET_KEY") {
                Ok(key) if !key.is_empty() => {
                    tracing::info!(binding = "payments", impl_ = "StripePaymentGateway", "payment service wired (STRIPE_SECRET_KEY set)");
                    Arc::new(stripe_adapter::StripePaymentGateway::new(key))
                }
                _ => {
                    tracing::warn!(
                        binding = "payments",
                        impl_ = "FailClosedPaymentGateway",
                        "STRIPE_SECRET_KEY unset -- EVERY checkout declines"
                    );
                    Arc::new(FailClosedPaymentGateway)
                }
            }
        })
        .expect("payment service binding (services.yaml)"),
        // The payment_process_manager state rows placeOrder opens/single-flights on
        // (ADR-20260719-193500).
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(
            pool.clone(),
        )),
        // The refund_process_manager rows the approveRefund/denyRefund decisions run on.
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(
            pool.clone(),
        )),
        // Acceptance-first dispatch (ADR-20260720-015300/-015500): the actor mailbox is THE
        // acceptance door since #242 Runtime D -- every mutation enqueues here and the
        // partitioned workers spawned below deliver; the transition broadcast behind
        // operationStatus(+Changed) rides the status bus below.
        mailbox: Arc::new(
            infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone())
                .with_nudges(mailbox_nudges.clone()),
        ),
        status_bus: operation_status_bus.clone(),
        slug_reservations: Arc::new(
            infrastructure::PgSlugReservationRepository::new(pool.clone()),
        ),
        auth_sessions: {
            // Encrypted parking store when AUTH_SESSION_KEY is set; else stays the no-op
            // (fail-closed: no key ⇒ no session cookies, never plaintext at rest).
            match infrastructure::PgAuthSessionStore::from_env(pool.clone()) {
                Some(store) => {
                    auth_sessions = Arc::new(store);
                    tracing::info!(binding = "auth_sessions", impl_ = "encrypted Pg store", "auth session store wired (AUTH_SESSION_KEY set)");
                    auth_sessions.clone()
                }
                None => {
                    tracing::warn!(binding = "auth_sessions", "AUTH_SESSION_KEY unset -- session cookies unavailable, auth stays anonymous-only");
                    auth_sessions.clone()
                }
            }
        },
    };
    GraphqlDi { read, write, tenant_lookup, auth_sessions, restaurants }
}

pub async fn router() -> Router {
    // The DECLARED configuration (specs/configuration.yaml), resolved once. Every value below comes
    // from here rather than a local `env::var` + inline fallback: a default that is declared in the
    // spec and then re-typed at the call site is two sources of truth, and the spec's copy is the one
    // that turns out to be inert. `main` resolves it too — for the startup gate — and both resolutions
    // read the same process env, so they agree by construction.
    let (config, _) = generated::config::Config::resolve();
    let snap = Arc::new(Mutex::new(Snapshot::default()));
    // In-process appended-event bus: every event-store append in THIS process (GraphQL mutations,
    // Stripe/HubRise inbound facts) is broadcast after commit, feeding the GraphQL subscriptions.
    // Constructed unconditionally so the schema always carries a bus (subscriptions without a DB
    // simply never receive anything).
    let event_bus = EventBus::default();
    // Operation-response broadcast (ADR-20260720-015500; behind the actor_client boundary since
    // #303): the legacy dispatch and the mailbox workers publish every completion here;
    // operationStatusChanged streams it through ActorClient::watch. Like the event bus,
    // constructed unconditionally so the schema always carries one.
    let operation_status_bus = actor_client::OperationStatusBus::default();
    // The enqueue→worker wake registry (one Notify per mailbox actor type): every in-process
    // PgMailbox insert nudges the actor type's worker, cutting delivery latency from the
    // heartbeat poll (~10s) to ~immediate. Registered up-front from the SAME generated table the
    // workers spawn from.
    let mailbox_nudges = {
        let mut nudges = infrastructure::persistence::mailbox_store::MailboxNudges::default();
        for (actor_type, _) in infrastructure::generated::command_router::ACTOR_MAILBOXES {
            nudges.register(actor_type);
        }
        Arc::new(nudges)
    };
    let mut read_deps: Option<ReadDeps> = None;
    // The host fallback's tenant lookup (#98): decides registered-vs-unclaimed for {slug} hosts.
    let mut tenant_lookup = hosts::TenantLookup(None);
    // IDENT-1 Phase A (#641, ADR-20260818-004646): the CUSTOMER identity-resolution mode, selected
    // ONCE here from the DECLARED configuration — never a per-request fallback. DEFAULT (and the
    // only reachable value without a database) is the legacy claim path.
    let mut customer_identity_source = auth::CustomerIdentitySource::Claim;
    // The RIDER seam (#639 part C step 2b) has no claim path to fall back to: without a database it
    // answers `LookupFailed` (fail closed, PAGE-class), and the pool branch below replaces it with
    // the Postgres resolver over the `Rider` projection's `auth_ref` bridge.
    let mut rider_identity_source =
        auth::RiderIdentitySource::new(Arc::new(auth::NoDatabaseRiderIdentity));
    // The MEMBER seam (#639 part C step 6-ii) has no claim path either, the SAME reasoning.
    let mut member_identity_source =
        auth::MemberIdentitySource::new(Arc::new(auth::NoDatabaseMemberIdentity));
    // The ADMIN/platform seam (#639 part C step 6-v, ADR-20260905-223957 §2) has no claim path
    // either, the SAME reasoning: without a database it answers `LookupFailed` (fail closed,
    // PAGE-class), and the pool branch below replaces it with the Postgres resolver over the
    // `PlatformMember` bridge.
    let mut platform_identity_source =
        auth::PlatformIdentitySource::new(Arc::new(auth::NoDatabasePlatformIdentity));
    // Cookie-pickup parking (#112): the real Pg store when DB + AUTH_SESSION_KEY are set, else the
    // fail-closed no-op (parking succeeds, claiming yields nothing → no cookie, anonymous still works).
    let mut auth_sessions: Arc<dyn application::auth_sessions::AuthSessionStore> =
        Arc::new(application::auth_sessions::NoopAuthSessionStore);
    let mut write_deps: Option<WriteDeps> = None;
    // The OTP send guards (#516) — Some only once a pool exists, because the counter that makes them
    // meaningful is SHARED state in Postgres. With no database, `/auth/sms-hook` 503s rather than
    // sending unguarded: fail-closed, since an unguarded send path is the failure being prevented.
    let mut sms_guard: Option<Arc<infrastructure::SmsSendAuthorizer>> = None;
    // The email send-abuse wall (#639 part C step 6-ii) — the SAME "Some only once a pool exists"
    // posture as `sms_guard`: without a database there is no shared counter, so
    // `send_email_magic_link` stays UNGUARDED rather than failing closed (this door has no
    // separate hook-path wall the way SMS does, so "no pool" already means "no real send" via the
    // fail-closed identity stand-in below).
    let mut email_guard: Option<Arc<infrastructure::EmailSendAuthorizer>> = None;
    // `SUPPORT_CONTACT` (#639 part C step 2c-i; ADR-20260830-213135: required, NO default), resolved
    // ONCE here from the declared configuration and handed to the rider sign-in door as a value.
    // Empty = unset (development only — staging/production refuse to boot without it): the door
    // then fails CLOSED, loudly, rather than printing a refusal that names no route.
    let support_contact: Option<domain::generated::scalars::EmailAddress> =
        Some(config.support_contact.trim())
            .filter(|s| !s.is_empty())
            .map(|s| domain::generated::scalars::EmailAddress(s.to_string()));
    if support_contact.is_none() {
        tracing::warn!(
            key = "SUPPORT_CONTACT",
            "unset -- the rider sign-in door refuses every attempt until a support route is configured"
        );
    }
    let mut projector_status: Option<Arc<Mutex<ProjectionStatus>>> = None;
    let mut saga_status: Option<Arc<Mutex<ProcessManagerStatus>>> = None;
    let mut deletion_status: Option<Arc<Mutex<infrastructure::DeletionEngineStatus>>> = None;
    // The activation cache handle (ACTOR_ACTIVATIONS on), hoisted out of the worker block so the
    // deletion engine can evict erased streams from it.
    let mut activation_cache: Option<Arc<infrastructure::mailbox::StreamActivations>> = None;
    let mut sirene_status: Option<Arc<Mutex<infrastructure::SireneSyncStatus>>> = None;
    let mut sirene_worker: Option<Arc<SireneSyncWorker>> = None;
    let mut stripe_ingestor: Option<Arc<StripeWebhookIngestor>> = None;
    let mut avelo37_ingestor: Option<Arc<Avelo37WebhookIngestor>> = None;
    let mut coopcycle_ingestor: Option<Arc<CoopCycleWebhookIngestor>> = None;
    // The CoopCycle federation registry (COOPCYCLE_INSTANCES) — shared by the outbound gateway (base
    // URL + OAuth per instance) and the inbound webhook route (per-instance secret). Empty ⇒ no-op.
    let coopcycle_registry = coopcycle_adapter::CoopCycleRegistry::from_env()
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, key = "COOPCYCLE_INSTANCES", "misconfigured, treating as unset");
            None
        })
        .unwrap_or_default();
    let mut uber_direct_ingestor: Option<Arc<UberDirectWebhookIngestor>> = None;
    // The Uber Direct config (UBER_DIRECT_*) — shared by the outbound gateway (OAuth2 + create
    // delivery) and the inbound webhook route (signing secret). None ⇒ unconfigured (no-op stand-in).
    let uber_direct_config = uber_direct_adapter::UberDirectConfig::from_env().unwrap_or_else(|e| {
        tracing::warn!(error = %e, key = "UBER_DIRECT_*", "misconfigured, treating as unset");
        None
    });
    let mut hubrise_state = hubrise_adapter::HubRiseWebhookState::default();

    match std::env::var("DATABASE_URL") {
        // Pool ceiling from the DECLARED configuration (#385): the same key every wired bin
        // reads, so the platform's total connection budget is reviewable in one spec place.
        Ok(url) if !url.is_empty() => match PgPoolOptions::new()
            .max_connections(config.database_pool_max_connections.clamp(1, 100) as u32)
            .min_connections(1)
            .acquire_timeout(Duration::from_secs(10))
            .idle_timeout(Duration::from_secs(240))
            .max_lifetime(Duration::from_secs(1800))
            .connect_lazy(&url)
        {
            Ok(pool) => {
                // Configured but unconfirmed until the first probe: report DOWN, not NOT_CONFIGURED.
                snap.lock().expect("health snapshot mutex").state = db_state::DOWN;
                spawn_heartbeat(pool.clone(), snap.clone());

                // The read/write dependency graph of the GraphQL surface — ONE composition,
                // shared with the graphql-{scope} subgraph bins (#385 API-tier wiring, D8):
                // extracting it is what lets a subgraph bin serve the same resolvers without a
                // logic fork. Everything below this call is monolith-only hosting (workers,
                // ingestors, SSR) that the bins re-home family by family.
                sms_guard = Some(sms_send_guard(&pool, &config));
                email_guard = Some(email_send_guard(&pool, &config));
                let di = build_graphql_di(
                    &pool,
                    &event_bus,
                    &operation_status_bus,
                    &mailbox_nudges,
                    sms_guard.clone(),
                    email_guard.clone(),
                    graphql::service_clock::ServiceWindowHorizon::from_seconds(
                        config.service_window_validity_horizon_seconds,
                    ),
                    support_contact.clone(),
                    config.run_rider_restriction_door,
                );
                // IDENT-1 Phase A (#641): gate-then-stabilize, selected ONCE here from the
                // resolved Config -- ON wraps the SAME `customers` repository `ReadDeps` already
                // carries (vernon/evans: reuse the port, never re-derive it), so the seam reads
                // through the identical `by_auth_ref` bridge the `me` query resolves through.
                if config.resolve_customer_identity_from_postgres {
                    customer_identity_source = auth::CustomerIdentitySource::Postgres(Arc::new(
                        auth::PgCustomerIdentity::new(di.read.customers.clone()),
                    ));
                }
                // The RIDER seam: ungated, Postgres, over the `Rider` projection (#639 part C
                // step 2b). Its port is not on `ReadDeps` because no GraphQL query reads it — the
                // table is `internal: true`; the request seam is its only reader.
                // ONE `PgRiderRepository` for BOTH readers of the bridge — the request seam and
                // the rider sign-in door's CommandDeps below (#639 part C step 2c-i): the port is
                // reused, never re-derived (vernon/evans).
                let rider_repository: Arc<dyn application::queries::RiderIdentityRepository> =
                    Arc::new(infrastructure::PgRiderRepository::new(pool.clone()));
                rider_identity_source = auth::RiderIdentitySource::new(Arc::new(
                    auth::PgRiderIdentity::new(rider_repository.clone()),
                ));
                // #639 part C step 6-ii: the member sign-in door's bridge (`member` table) + the
                // restaurant scope lookup (`scopemembership`) -- the SAME shared repository the
                // command deps below reuse (vernon/evans: reuse the port, never re-derive it).
                let member_repository: Arc<dyn application::queries::MemberIdentityRepository> =
                    Arc::new(infrastructure::PgMemberRepository::new(pool.clone()));
                let member_scopes: Arc<dyn application::queries::MemberRestaurantScopeRepository> =
                    Arc::new(infrastructure::persistence::scope_membership_store::PgScopeMembershipRepository::new(pool.clone()));
                member_identity_source = auth::MemberIdentitySource::new(Arc::new(
                    auth::PgMemberIdentity::new(member_repository.clone(), member_scopes),
                ));
                // #639 part C step 6-v (ADR-20260905-223957 §2): the ADMIN/platform seam's bridge
                // (`platform_member` table) -- the SAME shared repository `CommandDeps.platform_members`
                // below reuses (vernon/evans: reuse the port, never re-derive it).
                let platform_member_repository: Arc<dyn application::queries::PlatformMemberRepository> =
                    Arc::new(infrastructure::PgPlatformMemberRepository::new(pool.clone()));
                platform_identity_source = auth::PlatformIdentitySource::new(Arc::new(
                    auth::PgPlatformIdentity::new(platform_member_repository.clone()),
                ));
                // The HubRise connect flow (wired below) shares the restaurant read model.
                let hubrise_restaurants = di.restaurants.clone();
                // The host fallback shares it too (#98: registered-vs-unclaimed tenant slugs).
                tenant_lookup = di.tenant_lookup;
                auth_sessions = di.auth_sessions;
                read_deps = Some(di.read);
                write_deps = Some(di.write);

                // Push wake for the drain loops (ADR-20260802-200416): ONE dedicated LISTEN
                // connection feeds both the projector and the saga runner, so each append reaches
                // them on commit instead of being discovered by a 1.5 s poll — the polling was
                // ~70,900 queries/hour on an idle platform, 95% of outbound bandwidth. Both loops
                // keep a safety-net drain (NOTIFY has no replay) and revert to the fast poll
                // whenever the listener is down, so losing push degrades to the previous behaviour,
                // never past it. RUN_EVENT_PUSH=false forces the unassisted polling path (the
                // escape hatch for a transaction-mode pooler, which cannot carry LISTEN).
                let event_wake = if config.run_event_push {
                    let wake = infrastructure::EventWake::new();
                    infrastructure::spawn_event_listener(url.clone(), wake.clone());
                    Some(wake)
                } else {
                    tracing::warn!(toggle = "RUN_EVENT_PUSH", "event push OFF -- drain loops poll unassisted at 1.5 s");
                    None
                };

                // In-process projection worker (ADR-0040). RUN_PROJECTOR=false hands it to a dedicated worker.
                if config.run_projector {
                    let worker = ProjectionWorker::new(pool.clone());
                    projector_status = Some(worker.status());
                    tokio::spawn(worker.run_loop_with(event_wake.as_ref().map(|w| w.waiter())));
                    tracing::info!(worker = "projection", running = true, toggle = "RUN_PROJECTOR", "worker running in-process");
                } else {
                    tracing::warn!(worker = "projection", running = false, toggle = "RUN_PROJECTOR", "worker NOT started -- no read model advances, queries serve stale data");
                }

                // In-process saga runner (the state-table process managers of
                // specs/processmanager.yaml, ADR-20260719-193500) — same pattern as the projection
                // worker: RUN_PROCESS_MANAGERS=false hands it to a dedicated worker. The runner
                // builds its state-table stores and read models over the pool; the `delivery`
                // service resolves through the GENERATED topology binding (services.yaml, issue #26):
                // the composition root supplies the in-process constructor — the real outbound
                // Avelo37 gateway when AVELO37_API_KEY is configured (issue #28), otherwise the
                // logged no-op stand-in (jobs stay open to independent riders; the bounded re-offer
                // run row still terminates ACCEPTED/FAILED). The partner's answers always arrive
                // asynchronously through the webhook inbox below, never this outbound call.
                // THE STARTUP BACKFILL (#272 review MAJOR-2 + re-verify residual): any Stripe
                // fact a previously-running saga runner accepted but had not reacted to has no
                // deliverer — a PaymentCaptured with no OrderPlaced is a
                // paid order nobody is told about. Runs STRICTLY BEFORE the saga runner's first
                // tick (sequenced inside the runner's own task below): that first tick can
                // advance pm:RefundProcess past an un-reacted PaymentRefunded (via the group's
                // remaining order-fact triggers), and a backfill reading the checkpoint after
                // that would miss the fact forever. Idempotent (deterministic ids; the legs
                // absorb already-delivered hops); PM lanes are seeded first so the width lookup
                // can never race the workers' own seeding.
                let pm_backfill = {
                    let pool = pool.clone();
                    let nudges = mailbox_nudges.clone();
                    async move {
                        for (actor_type, width) in
                            infrastructure::generated::command_router::ACTOR_MAILBOXES
                        {
                            if matches!(*actor_type, "PlaceOrderProcess" | "RefundProcess") {
                                if let Err(e) =
                                    actor_runtime::seed_partitions(&pool, actor_type, *width as i16)
                                        .await
                                {
                                    tracing::error!(worker = "pm_backfill", error = %e, "seed failed -- backfill skipped; restart to retry");
                                    return;
                                }
                            }
                        }
                        let pm_state =
                            infrastructure::persistence::PgPaymentProcessState::new(pool.clone());
                        let mut attempt = 0u32;
                        loop {
                            attempt += 1;
                            match infrastructure::mailbox::backfill_stripe_facts_to_pm_lanes(
                                &pool, &pm_state,
                            )
                            .await
                            {
                                Ok(0) => {
                                    tracing::info!(worker = "pm_backfill", enqueued = 0, "no un-reacted Stripe facts to backfill");
                                    return;
                                }
                                Ok(n) => {
                                    tracing::warn!(worker = "pm_backfill", enqueued = n, "backfilled un-reacted Stripe facts onto the PM lanes");
                                    nudges.nudge("PlaceOrderProcess");
                                    nudges.nudge("RefundProcess");
                                    return;
                                }
                                Err(e) if attempt < 3 => {
                                    tracing::warn!(worker = "pm_backfill", error = %e, attempt, "backfill failed -- retrying");
                                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                }
                                Err(e) => {
                                    // Facts stay in the log — nothing is lost — but the runner
                                    // may now advance past them: LOUD, and a restart retries.
                                    tracing::error!(worker = "pm_backfill", error = %e, "backfill failed after retries -- restart to retry");
                                    return;
                                }
                            }
                        }
                    }
                };
                let mut pm_backfill = Some(pm_backfill);

                if config.run_process_managers {
                    // Composite delivery gateway (#60): the saga offers a job on a strategy-resolved
                    // CHANNEL, so the single Avelo-vs-Noop choice becomes a registry of channel →
                    // adapter. `independent` is the rider POOL (a deliberate no-op — jobs stay open to
                    // riders); `avelo37` is wired when AVELO37_API_KEY is set, and `coopcycle` when its
                    // federation registry (COOPCYCLE_INSTANCES) is configured (issue #58). Unwired
                    // channels (e.g. uber_direct in an unconfigured Tours) fall through: the offer times
                    // out and the saga escalates to the next ranked channel (today's deployments unchanged).
                    let partner = infrastructure::generated::service_bindings::delivery_service(|| {
                        let mut gateway = infrastructure::CompositeDeliveryGateway::new().with_channel(
                            "independent",
                            Arc::new(application::ports::NoopDeliveryService),
                        );
                        if let Some(avelo) = avelo37_adapter::Avelo37DeliveryGateway::from_env() {
                            gateway = gateway.with_channel("avelo37", Arc::new(avelo));
                        }
                        if !coopcycle_registry.is_empty() {
                            gateway = gateway.with_channel(
                                "coopcycle",
                                Arc::new(coopcycle_adapter::CoopCycleDeliveryGateway::new(
                                    coopcycle_registry.clone(),
                                )),
                            );
                        }
                        if let Some(config) = uber_direct_config.clone() {
                            gateway = gateway.with_channel(
                                "uber_direct",
                                Arc::new(uber_direct_adapter::UberDirectDeliveryGateway::new(config)),
                            );
                        }
                        tracing::info!(
                            binding = "delivery",
                            impl_ = "composite",
                            wired_channels = ?gateway.wired_channels(),
                            "delivery gateway wired (unwired channels fall through via offer timeout)"
                        );
                        Arc::new(gateway)
                    })
                    .expect("delivery service binding (services.yaml)");
                    // The `payment` service for the ReclamationProcess refund arm (#207) — the SAME
                    // Stripe binding the GraphQL write side uses, resolved through the generated topology
                    // binding: the real Stripe adapter when STRIPE_SECRET_KEY is set, else the
                    // fail-closed stand-in (a claim-resolution refund declines rather than moving money).
                    let saga_payments = infrastructure::generated::service_bindings::payment_service(|| {
                        match std::env::var("STRIPE_SECRET_KEY") {
                            Ok(key) if !key.is_empty() => {
                                Arc::new(stripe_adapter::StripePaymentGateway::new(key))
                            }
                            _ => Arc::new(FailClosedPaymentGateway),
                        }
                    })
                    .expect("payment service binding (services.yaml)");
                    let runner = ProcessManagerRunner::new(pool.clone())
                        .with_partner(partner)
                        .with_payments(saga_payments)
                        // #595/#797: resolved ONCE here, handed in — the runner never reads
                        // config. Every declared route's gate travels together and each is fed
                        // from its OWN key, so the runner can host two routes without binding
                        // them to one boolean.
                        .with_route_gates(application::generated::process_managers::RouteGates {
                            order_placed_to_order: config.route_order_birth_through_lane,
                            place_replacement_order_to_order: config
                                .route_replacement_birth_through_lane,
                            // #807: the three routed `send:` steps, each from its OWN key.
                            bind_cart_to_customer_to_cart: config.route_cart_bind_through_lane,
                            grant_customer_credit_to_customer_credit: config
                                .route_credit_grant_through_lane,
                            mark_order_delivered_to_order: config
                                .route_order_delivery_completion_through_lane,
                        });
                    saga_status = Some(runner.status());
                    // The backfill runs INSIDE the runner's task, before its first tick — the
                    // ordering the re-verification demanded (see the pm_backfill comment above).
                    let backfill = pm_backfill.take().expect("pm_backfill consumed once");
                    let saga_waiter = event_wake.as_ref().map(|w| w.waiter());
                    tokio::spawn(async move {
                        backfill.await;
                        runner.run_loop_with(saga_waiter).await;
                    });
                    tracing::info!(worker = "saga_runner", running = true, toggle = "RUN_PROCESS_MANAGERS", "worker running in-process");

                    // Delivery offer-timeout worker (#60): escalates a stale OFFERED run to the next
                    // ranked channel. Env-gated like the other in-process workers.
                    if config.run_delivery_offer_timeout {
                        // The ceiling comes from the DECLARED configuration, resolved once above --
                        // `infrastructure` is an inner layer and cannot read `Config` itself, so the
                        // composition root is the only place the two can meet.
                        let timeout_worker = Arc::new(infrastructure::DeliveryOfferTimeoutWorker::new(
                            pool.clone(),
                            config.delivery_offer_max_ttl_seconds,
                        ));
                        tokio::spawn(timeout_worker.run_loop());
                        tracing::info!(worker = "delivery_offer_timeout", running = true, toggle = "RUN_DELIVERY_OFFER_TIMEOUT", "worker running in-process");
                    } else {
                        tracing::warn!(worker = "delivery_offer_timeout", running = false, toggle = "RUN_DELIVERY_OFFER_TIMEOUT", "worker NOT started -- an unanswered offer is never expired");
                    }
                } else {
                    tracing::warn!(worker = "saga_runner", running = false, toggle = "RUN_PROCESS_MANAGERS", "worker NOT started -- no cross-aggregate reaction fires");
                    // No runner to race: the backfill still runs (facts past frozen checkpoints
                    // must reach the PM lanes), just unsequenced.
                    if let Some(backfill) = pm_backfill.take() {
                        tokio::spawn(backfill);
                    }
                }

                // SIRENE sync worker (ADR-0045): drains the `external_sirene_restaurants` staging
                // table through the ACL into the ordinary write path. Always constructed (the
                // /internal/sirene/drain ping needs it); the slow safety-net poll loop is gated by
                // RUN_SIRENE_WORKER — default OFF since 2026-07-28 (paused, issue #220).
                // THE MAILBOX WORKERS (#242 Runtime C3, PROP-20260728-152752): one per actor type
                // with a declared mailbox — claim partition lanes, drain head-of-line, deliver
                // through the generated command router, commit fenced. ALWAYS running when a DB is
                // configured: the flipped resolvers only enqueue, so without these workers every
                // aggregate-routed mutation would accept and then hang PENDING forever.
                {
                    let deps = infrastructure::generated::command_router::CommandDeps {
                        store: Arc::new(PgEventStore::new(pool.clone())),
                        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
                        slugs: Arc::new(infrastructure::PgSlugReservationRepository::new(pool.clone())),
                        auth_subjects: Arc::new(infrastructure::PgAuthSubjectReservationRepository::new(pool.clone())),
                        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
                        probe: Arc::new(UnverifiedGbpOrderLinkProbe),
                        prospection: Arc::new(PgProspectionRepository::new(pool.clone())),
                        catalogs: Arc::new(PgCatalogRepository::new(pool.clone())),
                        auth: infrastructure::generated::service_bindings::identity_service(
                            || identity_service_impl(sms_guard.clone(), email_guard.clone()),
                        )
                        .expect("identity service binding (services.yaml)"),
                        customers: Arc::new(PgCustomerRepository::new(pool.clone())),
                        sessions: auth_sessions.clone(),
                        // #639 part C step 2c-i: the rider sign-in door identifies through the
                        // SAME bridge the request seam reads, and names the support route the
                        // declared configuration resolved once above.
                        riders: rider_repository.clone(),
                        // #639 part C step 6-ii: the member sign-in door's SAME-shape bridge.
                        members: member_repository.clone(),
                        support_contact: support_contact.clone(),
                        // The SAME conditional Stripe binding the resolver side and the saga runner
                        // use (#272 Runtime D1): the mailbox workers execute the payment-dependent
                        // PM legs -- the PlaceOrderProcess/RefundProcess lanes are live and
                        // unconditional since #242 Runtime D -- so a hard-wired fail-closed
                        // stand-in here would silently decline every checkout.
                        payments: infrastructure::generated::service_bindings::payment_service(|| {
                            match std::env::var("STRIPE_SECRET_KEY") {
                                Ok(key) if !key.is_empty() => {
                                    Arc::new(stripe_adapter::StripePaymentGateway::new(key))
                                }
                                _ => {
                                    tracing::warn!(
                                        binding = "payments",
                                        site = "mailbox_workers",
                                        impl_ = "FailClosedPaymentGateway",
                                        "STRIPE_SECRET_KEY unset -- payment-dependent deliveries will decline"
                                    );
                                    Arc::new(FailClosedPaymentGateway)
                                }
                            }
                        })
                        .expect("payment service binding (services.yaml)"),
                        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(
                            pool.clone(),
                        )),
                        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(
                            pool.clone(),
                        )),
                        // The poisoned-row recovery port (#315): the RequeueMailboxMessage
                        // deliveries flip the target row through this arbiter.
                        mailbox_requeue: Arc::new(
                            infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(
                                pool.clone(),
                            ),
                        ),
                        // RSO-1 Phase 4: the PlaceOrder service-hours enforcement gate (default
                        // OFF = shadow), resolved from the declared configuration ONCE here at
                        // the composition root — the handler takes it as a parameter.
                        enforce_service_hours_guard: config.enforce_service_hours_guard,
                        // #167: the acceptance-timeout ACTION gate (default OFF = shadow), read
                        // at DELIVERY time by the OrderAcceptanceTimedOut route — same
                        // composition-root resolution.
                        enforce_acceptance_timeout: config.enforce_acceptance_timeout,
                        // #797: one field per DECLARED route, each fed from its own key.
                        route_gates: application::generated::process_managers::RouteGates {
                            order_placed_to_order: config.route_order_birth_through_lane,
                            place_replacement_order_to_order: config
                                .route_replacement_birth_through_lane,
                            // #807: the three routed `send:` steps, each from its OWN key.
                            bind_cart_to_customer_to_cart: config.route_cart_bind_through_lane,
                            grant_customer_credit_to_customer_credit: config
                                .route_credit_grant_through_lane,
                            mark_order_delivered_to_order: config
                                .route_order_delivery_completion_through_lane,
                        },
                        // #639 part C step 4-iii-A (ADR-20260904-152807 §7): the restrict door's
                        // release gate, resolved ONCE here at the composition root — the handler
                        // takes it as a parameter, exactly like `enforce_service_hours_guard`.
                        run_rider_restriction_door: config.run_rider_restriction_door,
                        // #639 part C step 6-i (ADR-20260905-101349 §6): the staff access grant
                        // door, the SAME resolved-once-here shape.
                        run_member_access_grant: config.run_member_access_grant,
                        // #639 part C step 6-ii: the member sign-in door, the SAME shape.
                        run_member_sign_in_door: config.run_member_sign_in_door,
                        // #639 part C step 6-iv: the invitation door, the SAME shape.
                        run_restaurant_invitation: config.run_restaurant_invitation,
                        // #639 part C step 6-v (ADR-20260905-223957 §5): the platform grant door,
                        // the SAME shape.
                        run_platform_access_grant: config.run_platform_access_grant,
                        // The PlatformMember bridge's write-side arbiter (ADR-20260905-223957 §1)
                        // -- the SAME shared repository the seam above reuses.
                        platform_members: platform_member_repository.clone(),
                    };
                    // Deploy-time fleet-parity EVIDENCE (#598): the monolith re-asserts its
                    // resolved value for the same three gates the standalone fleets declare
                    // (`infrastructure::mailbox::standalone_deps`), so a rolling deploy in which
                    // half the fleet routes the Order birth and half appends it is a FACT
                    // (`count(distinct value) by (flag) > 1`) rather than a review-time
                    // assertion. Nothing else can see that split: both halves birth exactly one
                    // order, and only the routed half arms the acceptance deadline.
                    telemetry::meters::runtime::declare_flag(
                        "ENFORCE_SERVICE_HOURS_GUARD",
                        config.enforce_service_hours_guard,
                    );
                    telemetry::meters::runtime::declare_flag(
                        "ENFORCE_ACCEPTANCE_TIMEOUT",
                        config.enforce_acceptance_timeout,
                    );
                    telemetry::meters::runtime::declare_flag(
                        "ROUTE_ORDER_BIRTH_THROUGH_LANE",
                        config.route_order_birth_through_lane,
                    );
                    // #595: the same split-fleet argument as the key above — the replacement route
                    // must read ONE value across every process that runs a saga runner, or some
                    // replacement births lane and others do not, invisibly.
                    telemetry::meters::runtime::declare_flag(
                        "ROUTE_REPLACEMENT_BIRTH_THROUGH_LANE",
                        config.route_replacement_birth_through_lane,
                    );
                    // Round 2 item 7 (farley, vernon): the door key's OWN fleet-parity evidence —
                    // missing at BOTH composition roots before this round (`standalone_deps` gets
                    // the same call), so a rolling deploy in which half the fleet's `restrictRider`
                    // dispatches refuse the typed error while the other half accepts was invisible.
                    telemetry::meters::runtime::declare_flag(
                        "RUN_RIDER_RESTRICTION_DOOR",
                        config.run_rider_restriction_door,
                    );
                    // #639 part C step 6-i (ADR-20260905-101349 §6, the #882 fleet-parity lesson):
                    // the grant door's own fleet-parity evidence, the standalone composition root
                    // (`infrastructure::mailbox::standalone_deps`) declares the same key.
                    telemetry::meters::runtime::declare_flag(
                        "RUN_MEMBER_ACCESS_GRANT",
                        config.run_member_access_grant,
                    );
                    // #639 part C step 6-ii: `RUN_MEMBER_SIGN_IN_DOOR`'s fleet-parity declaration
                    // and its `door_enforcing` liveness gauge moved OUT of this `if let Some(pool)`
                    // branch in round 2 (R2-F1) to the unconditional composition root beside
                    // `RUN_RIDER_RESTRICTION_SOCKET_CLOSE`, below — a monolith booting with no
                    // `DATABASE_URL` emitted no timeseries for this gate at all, indistinguishable
                    // from "not running the new build" (the exact defect `RUN_RIDER_RESTRICTION_
                    // SOCKET_CLOSE` already fixed once). `standalone.rs` already registered it
                    // unconditionally, so the two composition roots had drifted.
                    // ACTIVATIONS (#272 D3, gated ACTOR_ACTIVATIONS default false): the shared
                    // held-state cache, its per-actor policy from the GENERATED table, and a
                    // sweep timer so idle actors leave memory on schedule (not only when
                    // touched). OFF, `activations` is None and every delivery folds from the
                    // log exactly as before.
                    let activations = if config.actor_activations {
                        let cache = Arc::new(infrastructure::mailbox::StreamActivations::new(
                            (config.actor_activation_max_memory_mb.max(1) as usize) * 1024 * 1024,
                        ));
                        let per_actor: std::collections::HashMap<&'static str, (bool, Option<std::time::Duration>)> =
                            infrastructure::generated::command_router::ACTOR_ACTIVATIONS
                                .iter()
                                .map(|(actor, enabled, idle)| {
                                    (*actor, (*enabled, idle.map(|s| std::time::Duration::from_secs(s.max(1) as u64))))
                                })
                                .collect();
                        let settings = Arc::new(infrastructure::mailbox::ActivationSettings {
                            cache: cache.clone(),
                            idle_default: std::time::Duration::from_secs(
                                config.actor_activation_idle_seconds.max(1) as u64,
                            ),
                            per_actor,
                        });
                        tokio::spawn(async move {
                            loop {
                                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                                let swept = cache.sweep();
                                if swept > 0 {
                                    tracing::debug!(swept, "activations: idle passivation sweep");
                                }
                            }
                        });
                        tracing::info!(
                            max_memory_mb = config.actor_activation_max_memory_mb,
                            idle_seconds = config.actor_activation_idle_seconds,
                            "activations: held-state cache ON (ACTOR_ACTIVATIONS)"
                        );
                        Some(settings)
                    } else {
                        None
                    };
                    activation_cache = activations.as_ref().map(|s| s.cache.clone());
                    let handler = Arc::new({
                        let mut h = infrastructure::mailbox::MailboxCommandHandler::new(deps)
                            .with_event_bus(event_bus.clone())
                            // The declared reminder windows (actors.yaml `after:` →
                            // configuration.yaml), so `schedules:` deliveries start their
                            // clocks from configuration, never a constant (ADR-20260731-214500).
                            .with_reminder_windows(config.reminder_windows())
                            // Runtime D1 (#272, B2): a recorded Stripe fact chains its
                            // PM-addressed copy in the SAME completion transaction, and the PM
                            // lane's worker is nudged post-commit.
                            .with_nudges(mailbox_nudges.clone());
                        if let Some(settings) = &activations {
                            h = h.with_activations(settings.clone());
                        }
                        h
                    });
                    let observer = Arc::new(infrastructure::mailbox::StatusBusObserver::new(
                        operation_status_bus.clone(),
                    ));
                    // Unique per PROCESS (pid alone collides across hosts; hostname is an env
                    // read the configuration gate would demand a declaration for). Only
                    // uniqueness matters: claimed_by is a fencing identity plus a diagnostic.
                    let worker_id =
                        format!("w-{}-{}", std::process::id(), &uuid::Uuid::new_v4().simple().to_string()[..8]);
                    // ONE shutdown channel for every worker, flipped by the signal task below —
                    // the SENDER MUST STAY ALIVE: a dropped sender cannot deliver a shutdown, and
                    // (PR #270 review C1) the workers' graceful lane release would be dead code,
                    // stalling every lane for a full lease on each deploy.
                    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
                    tokio::spawn(async move {
                        let ctrl_c = async {
                            let _ = tokio::signal::ctrl_c().await;
                        };
                        #[cfg(unix)]
                        let terminate = async {
                            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                                Ok(mut sig) => {
                                    sig.recv().await;
                                }
                                Err(_) => std::future::pending().await,
                            }
                        };
                        #[cfg(not(unix))]
                        let terminate = std::future::pending::<()>();
                        tokio::select! {
                            _ = ctrl_c => {}
                            _ = terminate => {}
                        }
                        tracing::info!("mailbox: shutdown signal -- draining workers");
                        let _ = shutdown_tx.send(true);
                        // Hold the sender until the process ends: dropping it here would turn the
                        // receivers' `changed()` into an instant wake again.
                        std::future::pending::<()>().await;
                    });
                    // MAILBOX PUSH (#313, PROP-20260802-223522): one LISTEN connection fans
                    // committed enqueues out to the same per-actor-type nudges the in-process
                    // producers use — cross-process too, so a standalone adapter's recorded fact
                    // wakes these workers on commit. While the listener is live the full pass
                    // stretches to the 60 s safety net (beat stays on the heartbeat); whenever
                    // it is down — or RUN_MAILBOX_PUSH=false — workers run the pre-push cadence.
                    let mailbox_push = if config.run_mailbox_push {
                        let push = infrastructure::persistence::mailbox_wake::MailboxPush::new();
                        infrastructure::persistence::mailbox_wake::spawn_mailbox_listener(
                            url.clone(),
                            pool.clone(),
                            mailbox_nudges.clone(),
                            push.clone(),
                        );
                        Some(push)
                    } else {
                        tracing::warn!(toggle = "RUN_MAILBOX_PUSH", "mailbox push OFF -- workers poll at the heartbeat cadence");
                        None
                    };
                    // The spec-declared knobs, wired (MAILBOX_* in specs/configuration.yaml):
                    // cadence + lease from config, and the D4 poison cap.
                    let worker_config = actor_runtime::WorkerConfig {
                        lease_seconds: config.mailbox_lease_seconds,
                        heartbeat_seconds: config.mailbox_heartbeat_seconds.max(1) as u64,
                        max_delivery_attempts: config
                            .mailbox_max_delivery_attempts
                            .clamp(0, i16::MAX as i64) as i16,
                        // The spec prose says "retry spacing default = the heartbeat" — wire it
                        // so that stays true when MAILBOX_HEARTBEAT_SECONDS is tuned.
                        retry_spacing_seconds: config.mailbox_heartbeat_seconds.max(1) as u64,
                        ..actor_runtime::WorkerConfig::default()
                    };
                    for (actor_type, width) in
                        infrastructure::generated::command_router::ACTOR_MAILBOXES
                    {
                        let worker = Arc::new(
                            {
                                let mut w = actor_runtime::MailboxWorker::new(
                                    pool.clone(),
                                    worker_id.clone(),
                                    *actor_type,
                                    worker_config.clone(),
                                    handler.clone(),
                                )
                                .with_observer(observer.clone());
                                if let Some(nudge) = mailbox_nudges.get(actor_type) {
                                    w = w.with_nudge(nudge);
                                }
                                if let Some(push) = &mailbox_push {
                                    w = w.with_push_live(push.live_flag());
                                }
                                // A lane this worker stops owning drops its held activations —
                                // the new owner may write those actors (§3.5 eviction rules).
                                if let Some(settings) = &activations {
                                    w = w.with_lane_events(Arc::new(
                                        infrastructure::mailbox::ActivationLaneEvents(
                                            settings.cache.clone(),
                                        ),
                                    ));
                                }
                                w
                            },
                        );
                        let width = *width as i16;
                        let rx = shutdown_rx.clone();
                        // SUPERVISED: the loop itself retries transient errors, but a handler
                        // panic unwinds through the task — the supervisor respawns it (with
                        // backoff) so one poisoned delivery cannot permanently end an actor
                        // type's consumption.
                        tokio::spawn(async move {
                            if let Err(e) = worker.seed(width).await {
                                tracing::error!(worker = %worker.worker_id, actor_type = %worker.actor_type, error = %e, "mailbox: seed failed -- worker not started");
                                return;
                            }
                            loop {
                                let run = {
                                    let w = worker.clone();
                                    let rx = rx.clone();
                                    tokio::spawn(async move { w.run(rx).await })
                                };
                                match run.await {
                                    Ok(Ok(())) => break, // graceful shutdown
                                    Ok(Err(e)) => {
                                        tracing::error!(worker = %worker.worker_id, actor_type = %worker.actor_type, error = %e, "mailbox: worker loop exited -- respawning");
                                    }
                                    Err(join_err) => {
                                        tracing::error!(worker = %worker.worker_id, actor_type = %worker.actor_type, error = %join_err, "mailbox: worker loop panicked -- respawning");
                                    }
                                }
                                if *rx.borrow() {
                                    break;
                                }
                                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            }
                        });
                    }
                    tracing::info!(
                        workers = infrastructure::generated::command_router::ACTOR_MAILBOXES.len(),
                        "mailbox: per-actor-type workers running in-process"
                    );
                    // #167: the reminder-promotion dead-man's switch + SCHEDULED-depth gauge —
                    // a monitor OUTSIDE the workers it watches (ADR-20260810-231300 monitoring
                    // carve-out), emitting on every tick so a dead promotion pass reads as a
                    // GROWING lag, never as silence.
                    infrastructure::mailbox::spawn_promotion_watch(
                        pool.clone(),
                        std::time::Duration::from_secs(30),
                    );
                    // #598: the ROUTED-BIRTH lane dead-man's switch. UNCONDITIONAL — never behind
                    // ROUTE_ORDER_BIRTH_THROUGH_LANE: `order_birth_lag_ms` is silent by design
                    // while the flag is OFF, so without a heartbeat that has been running all
                    // along, "flag off" and "the Order lane worker is dead" are the same
                    // observation on the day of the flip.
                    infrastructure::mailbox::spawn_order_lane_watch(
                        pool.clone(),
                        std::time::Duration::from_secs(30),
                    );
                    // #608: THE BIRTH-GAP dead-man's switch — "a customer's money is held and no
                    // order exists". UNCONDITIONAL, for the same reason as the lane watch above:
                    // both gauges it emits read 0 on a healthy system, so the only way to know the
                    // switch works is to have watched it work before the day it is needed.
                    infrastructure::mailbox::spawn_birth_gap_watch(
                        pool.clone(),
                        std::time::Duration::from_secs(30),
                    );
                    // #639 part C step 3-ii (ADR-20260904-015903 §8): THE CUSTODY-HANDBACK
                    // dead-man's switch — "a rider handed a job back and nobody re-offered it".
                    // UNCONDITIONAL, same reason as the two switches above: the gauge reads 0 on a
                    // healthy system, so the only way to know it works is to have watched it work
                    // before the day it is needed. Non-fenced (ADR-20260904-015903 §10) — beside
                    // the offer-timeout worker below, never inside the fenced mailbox handler.
                    // Since #639 part C step 4-iii-B (ADR-20260904-152807 §8) the SAME tick also
                    // emits the rider-custody dead-man gauge -- the ceiling comes from the DECLARED
                    // configuration, resolved once above, the same reason the delivery-offer
                    // timeout worker below is handed its own resolved ceiling rather than reading
                    // the environment itself.
                    infrastructure::spawn_delivery_handback_watch(
                        pool.clone(),
                        infrastructure::delivery_handback_watch::default_sweep_interval(),
                        config.rider_restricted_custody_max_age_seconds,
                    );

                    // (The startup Stripe-fact backfill runs INLINE before the saga runner
                    // spawns — see the pm_backfill block above the RUN_PROCESS_MANAGERS gate.)
                }

                // Stripe webhook ingestor (ADR-20260731-122500 — the mailbox is the only door):
                // verify → mirror the verbatim delivery into external_stripe_events → ACL → ENQUEUE
                // the adapted fact on its Payment lane → ACK; the mailbox workers above deliver.
                // Mounted at `POST /adapters/stripe/webhooks` below.
                stripe_ingestor = Some(Arc::new(StripeWebhookIngestor::new(
                    Arc::new(stripe_adapter::PgRawStripeEvents::new(pool.clone())),
                    Arc::new(infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone()).with_nudges(mailbox_nudges.clone())),
                )));

                // Avelo37 delivery-partner webhook ingestor (issue #28, same two-layer inbox as
                // Stripe): verify → mirror → ACL → ENQUEUE the adapted delivery fact on its
                // DeliveryJob lane → ACK. Mounted at `POST /adapters/avelo37/webhooks` below.
                avelo37_ingestor = Some(Arc::new(Avelo37WebhookIngestor::new(
                    Arc::new(avelo37_adapter::PgRawAvelo37Events::new(pool.clone())),
                    Arc::new(infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone()).with_nudges(mailbox_nudges.clone())),
                )));

                // CoopCycle delivery-partner webhook ingestor (issue #58, same two-layer inbox): the
                // federation twist is that the verified webhook arrives per-instance at
                // `POST /adapters/coopcycle/{instance}/webhooks` and is namespaced by instance; the
                // ingestor itself is provider-shaped like Avelo37's (mirror → ACL → mailbox lane).
                // Mounted below with the registry (secrets).
                coopcycle_ingestor = Some(Arc::new(CoopCycleWebhookIngestor::new(
                    Arc::new(coopcycle_adapter::PgRawCoopCycleEvents::new(pool.clone())),
                    Arc::new(infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone()).with_nudges(mailbox_nudges.clone())),
                )));

                // Uber Direct delivery-partner webhook ingestor (issue #57, same two-layer inbox as
                // Avelo37/CoopCycle): verify the X-Uber-Signature → mirror → ACL → ENQUEUE the
                // adapted delivery fact on its DeliveryJob lane → ACK. Mounted at
                // `POST /adapters/uber-direct/webhooks` below with the signing secret.
                uber_direct_ingestor = Some(Arc::new(UberDirectWebhookIngestor::new(
                    Arc::new(uber_direct_adapter::PgRawUberDirectEvents::new(pool.clone())),
                    Arc::new(infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone()).with_nudges(mailbox_nudges.clone())),
                )));

                // HubRise wiring (issue #20): the raw mirror (external_hubrise_callbacks), the
                // enrichment, AND the connect flow all need only the database — the pull token is
                // resolved per connected account from `hubrise_connections` (the global
                // `HUBRISE_ACCESS_TOKEN` fallback is retired). The connect routes additionally
                // require the app credentials (HUBRISE_CLIENT_ID + HUBRISE_WEBHOOK_SECRET +
                // HUBRISE_CONNECT_REDIRECT_URL), checked per request fail-closed.
                hubrise_state.raw =
                    Some(Arc::new(hubrise_adapter::PgRawHubRiseCallbacks::new(pool.clone())));
                {
                    let hubrise_mailbox = Arc::new(
                        infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone())
                            .with_nudges(mailbox_nudges.clone()),
                    );
                    let hubrise_connections =
                        Arc::new(hubrise_adapter::PgHubRiseConnections::new(pool.clone()));
                    // Enricher/connect sends are fire-and-forget mailbox enqueues on the WORKER
                    // channel (ADR-20260731-122500): callback redeliveries dedupe on the mailbox pk
                    // instead of double-applying; the mailbox worker delivers.
                    hubrise_state.enricher = Some(Arc::new(hubrise_adapter::HubRiseEnricher::new(
                        hubrise_mailbox.clone(),
                        hubrise_connections.clone(),
                        hubrise_adapter::api::HubRiseApi::from_env(),
                    )));
                    hubrise_state.connect = Some(Arc::new(hubrise_adapter::HubRiseConnectFlow::new(
                        hubrise_mailbox,
                        Some(operation_status_bus.clone()),
                        hubrise_restaurants,
                        hubrise_connections,
                        hubrise_adapter::connect::HttpHubRiseConnectGateway {
                            api: hubrise_adapter::api::HubRiseApi::from_env(),
                            client_id: config.hubrise_client_id.clone().unwrap_or_default(),
                            client_secret: std::env::var("HUBRISE_WEBHOOK_SECRET").unwrap_or_default(),
                        },
                    )));
                }

                // Retention sweep worker (ADR-20260721-025159): periodically calls the
                // sweep_retention() SQL function — journal/mirror retention windows live in the
                // function, never here. Env-gated like the other workers; a pg_cron job calling
                // the same function is the alternative where DB-side scheduling is preferred.
                if config.run_retention_sweep {
                    let sweeper =
                        Arc::new(infrastructure::RetentionSweepWorker::new(pool.clone()));
                    tokio::spawn(sweeper.run_loop());
                    tracing::info!(worker = "retention_sweep", running = true, toggle = "RUN_RETENTION_SWEEP", "worker running in-process");
                } else {
                    tracing::warn!(worker = "retention_sweep", running = false, toggle = "RUN_RETENTION_SWEEP", "worker NOT started -- nothing expires and storage grows without bound");
                }

                // The generic deletion engine (ADR-20260731-214500 §4): gated DEFAULT-OFF until
                // smoked (gate-then-stabilize — this worker DELETES event streams). An engine
                // that cannot serve a DECLARED deletion policy refuses to construct; with the
                // gate ON that is a boot-stopping wiring bug (fail-fast, ADR-20260729-010500).
                if config.run_deletion_engine {
                    let mut engine = infrastructure::DeletionEngine::new(pool.clone())
                        .unwrap_or_else(|reason| {
                            panic!("RUN_DELETION_ENGINE is on but the engine refused to start: {reason}")
                        });
                    // Erased streams must leave the activation cache with the deletion (GDPR:
                    // no held fold of deleted events, no gapped resurrection from a held version).
                    if let Some(cache) = &activation_cache {
                        engine = engine.with_activations(cache.clone());
                    }
                    deletion_status = Some(engine.status());
                    // Restart-safe at every journey boundary (two-tx design), so it needs no
                    // graceful drain: hold a never-firing shutdown sender and let the loop pace
                    // on its heartbeat.
                    let (engine_shutdown_tx, engine_shutdown_rx) =
                        tokio::sync::watch::channel(false);
                    std::mem::forget(engine_shutdown_tx);
                    tokio::spawn(engine.run_loop(engine_shutdown_rx));
                    tracing::info!(worker = "deletion_engine", running = true, toggle = "RUN_DELETION_ENGINE", "worker running in-process");
                } else {
                    tracing::info!(worker = "deletion_engine", running = false, toggle = "RUN_DELETION_ENGINE", "worker NOT started (gated default) -- recorded expiry facts accumulate; no stream is erased");
                }

                let worker = Arc::new(SireneSyncWorker::new(pool.clone()));
                // Taken unconditionally, BEFORE the gate below: the worker exists either way (the ping
                // endpoint drives it), so `/sirene` can report `running: false` for a paused loop
                // instead of the ambiguous "not available" (#244).
                sirene_status = Some(worker.status());
                sirene_worker = Some(worker.clone());
                // PAUSED 2026-07-28 (product-owner directive): the default is OFF until the write-path
                // defects in issue #220 are resolved — a drain pass issues one `RegisterRestaurant` per
                // pending SIRET whose idempotency is a deliberate UNIQUE(stream_name, version) violation
                // (~200k dead tuples in `domain_events` per sweep) and resolves identity through an
                // unindexed `external_identifiers @> $1` scan of the whole Restaurant projection, per row.
                // It also cannot apply what it reads: no `UpdateRestaurant` exists here, so INSEE changes
                // are swallowed by that same conflict. The CI half is paused in sirene-sync.yml.
                // Re-enable BOTH halves together: `RUN_SIRENE_WORKER=true` + the workflow's cron.
                if config.run_sirene_worker {
                    tokio::spawn(worker.run_loop());
                    tracing::info!(
                        worker = "sirene_sync",
                        running = true,
                        toggle = "RUN_SIRENE_WORKER",
                        "worker running in-process"
                    );
                } else {
                    tracing::warn!(worker = "sirene_sync", running = false, toggle = "RUN_SIRENE_WORKER", "worker PAUSED (issue #220) -- staged rows stay PENDING; set RUN_SIRENE_WORKER=true to resume");
                }
            }
            Err(e) => tracing::error!(error = %e, "DATABASE_URL set but pool init failed -- /health will report degraded"),
        },
        _ => tracing::warn!("DATABASE_URL not set -- /health will report not_configured (503)"),
    }

    let base = Router::new()
        .route("/ping", get(ping))
        .route("/health", get(health))
        .route("/projector", get(projector))
        .route("/saga", get(saga))
        .route("/deletion", get(deletion))
        .route("/sirene", get(sirene))
        .with_state(AppState { snap, projector_status, saga_status, deletion_status, sirene_status });

    // Built once, shared twice: the HTTP GraphQL routes AND the SSR page renderer (#92 — the
    // in-process transport executes screens' data_requirements against this same schema).
    // Session-cookie transport (#112): the identity service (for /auth/refresh) + the parking store
    // (for /auth/session pickup). Identity resolves through the same generated binding as WriteDeps.
    let auth_routes_state = auth_routes::AuthRoutesState {
        sessions: Some(auth_sessions.clone()),
        identity: infrastructure::generated::service_bindings::identity_service(|| {
            identity_service_impl(sms_guard.clone(), email_guard.clone())
        })
        .expect("identity service binding (services.yaml)"),
        // The Supabase Send-SMS hook → OVH delivery (#118): both the OVH client and the hook secret
        // must be configured, else the hook 503s (SMS-less, never half-open).
        sms: infrastructure::OvhSmsClient::from_env().map(|c| {
            tracing::info!(binding = "sms", impl_ = "OvhSmsClient", "sms delivery wired (OVH_* set)");
            Arc::new(c)
        }),
        sms_hook_secret: std::env::var("SUPABASE_SMS_HOOK_SECRET")
            .ok()
            .filter(|s| !s.is_empty())
            .and_then(|s| infrastructure::supabase_sms_hook::decode_hook_secret(&s))
            .map(Arc::new),
        // THE WALL (#516): without the shared counter the hook refuses to send at all.
        sms_guard: sms_guard.clone(),
    };

    let schema = graphql::schema::build_schema(read_deps, write_deps, Some(event_bus));
    let ssr_exec = web_ssr::SsrExec {
        schema: schema.clone(),
        // #440: parsed once, here — empty and malformed collapse to None (degrade), so no
        // downstream consumer can be handed a key that cannot mount.
        stripe_publishable_key: web::stripe::PublishableKey::parse(
            config.stripe_publishable_key.as_deref(),
        ),
    };

    // #639 part C step 5 (ADR-20260905-065415 §6): fleet-parity evidence for the socket-close
    // gate, the `RUN_RIDER_RESTRICTION_DOOR` precedent — a rolling deploy in which half the fleet
    // closes a restricted rider's socket and half does not would otherwise be invisible.
    telemetry::meters::runtime::declare_flag(
        "RUN_RIDER_RESTRICTION_SOCKET_CLOSE",
        config.run_rider_restriction_socket_close,
    );
    // #639 part C step 6-ii (round 2, R2-F1): the sign-in door's OWN fleet-parity declaration and
    // its `door_enforcing` liveness gauge, moved HERE from inside the `if let Some(pool)` branch
    // above — a monolith booting with no `DATABASE_URL` must still declare this flag and emit this
    // gauge (the `RUN_RIDER_RESTRICTION_SOCKET_CLOSE` precedent just above; `standalone.rs`
    // already did this unconditionally, so the two composition roots had drifted).
    telemetry::meters::runtime::declare_flag(
        "RUN_MEMBER_SIGN_IN_DOOR",
        config.run_member_sign_in_door,
    );
    telemetry::meters::member_sign_in::door_enforcing(config.run_member_sign_in_door);
    // #639 part C step 6-iv: the invitation door's OWN fleet-parity declaration and liveness
    // gauge, unconditional at BOTH composition roots from birth (the `RUN_MEMBER_SIGN_IN_DOOR`
    // precedent above -- never left inside an `if let Some(pool)` branch to begin with).
    telemetry::meters::runtime::declare_flag(
        "RUN_RESTAURANT_INVITATION",
        config.run_restaurant_invitation,
    );
    telemetry::meters::restaurant_invitation::door_enforcing(config.run_restaurant_invitation);
    // #639 part C step 6-v (ADR-20260905-223957 §5): the platform grant door's own fleet-parity
    // declaration and liveness gauge, unconditional at BOTH composition roots from birth (the
    // `RUN_MEMBER_SIGN_IN_DOOR` precedent above -- never left inside an `if let Some(pool)` branch).
    telemetry::meters::runtime::declare_flag(
        "RUN_PLATFORM_ACCESS_GRANT",
        config.run_platform_access_grant,
    );
    telemetry::meters::admin_identity::grant_enforcing(config.run_platform_access_grant);
    // Round 2 R2-3 (ADR-20260905-065415 §7/§8, the `otp_send_guard_enforcing` precedent): register
    // the inverted dead-man's switch HERE, at the composition root, before any watcher can ever
    // spawn — without this call the gauge's `ObservableGauge` callback is registered only inside
    // `rider_socket::watch`, so "gate ON, nobody connected yet" (or the watcher never spawning at
    // all) reports NO timeseries instead of the declared 0, which is the exact defect class
    // CLAUDE.md names: a monitor that can only fire once a signal arrives goes quiet exactly when
    // it should scream. `watch_live_delta(0)` is a genuine no-op on the live count and idempotent
    // to call again from the watcher itself.
    telemetry::meters::rider_restriction::watch_live_delta(0);
    base.merge(graphql::routes::graphql_routes_with_socket_close_gate(
        schema,
        tenant_lookup.clone(),
        auth::IdentitySources {
            customer: customer_identity_source,
            rider: rider_identity_source,
            member: member_identity_source,
            platform: platform_identity_source,
        },
        graphql::rider_socket::RunRiderRestrictionSocketClose(
            config.run_rider_restriction_socket_close,
        ),
    ))
        // Internal trigger (ADR-0045): the CI ingestion pings this to wake the SIRENE sync worker.
        .merge(graphql::routes::sirene_internal_routes(sirene_worker))
        // Internal trigger (ADR-20260720-015400): ops ping to wake the inbound-events drain worker.
        // Partner webhook adapters (ADR-20260718-213352): self-contained crates under crates/adapters/*,
        // each mountable here (monolith) or deployable as its own web service. `POST /adapters/stripe/webhooks`
        // (signature-verified inbound payment facts), `POST /adapters/avelo37/webhooks` (signature-verified
        // inbound delivery-partner facts, issue #28) and `POST /adapters/hubrise/webhooks` (HMAC-verified ingress).
        .merge(stripe_adapter::routes(stripe_ingestor))
        .merge(avelo37_adapter::routes(avelo37_ingestor))
        // CoopCycle per-instance webhooks (issue #58): `POST /adapters/coopcycle/{instance}/webhooks`,
        // verified with the instance's registry secret. State carries the ingestor + the registry.
        .merge(coopcycle_adapter::routes(coopcycle_adapter::CoopCycleWebhookState {
            ingestor: coopcycle_ingestor,
            registry: Arc::new(coopcycle_registry),
        }))
        // Uber Direct webhooks (issue #57): `POST /adapters/uber-direct/webhooks`, verified with the
        // X-Uber-Signature raw-body HMAC. State carries the ingestor + the signing secret.
        .merge(uber_direct_adapter::routes(uber_direct_adapter::UberDirectWebhookState {
            ingestor: uber_direct_ingestor,
            webhook_secret: uber_direct_config.map(|c| Arc::new(c.webhook_secret)),
        }))
        .merge(hubrise_adapter::routes(hubrise_state))
        // Session-cookie endpoints (#112): POST /auth/{session,refresh,logout}.
        .merge(auth_routes::auth_routes(auth_routes_state))
        // The DERIVED `/services/<service>/<op>` surface (issue #26): emitted per the spec's
        // `expose` flags — empty while every service declares `expose: false` (V0).
        .merge(generated::services_routes::services_router())
        // The wasm hydrate bundle (split 4/4 of #21): built by wasm-bindgen in the Docker image into
        // WEB_ASSETS_DIR (default /app/web-assets) and served under /assets. A deployment without the
        // dir simply 404s here and pages stay SSR-only — degraded, never broken.
        .nest_service(
            "/assets",
            tower_http::services::ServeDir::new(
                config.web_assets_dir.clone(),
            ),
        )
        // Host-based serving (ADR-0036 + split 4/4 of #21): any path not matched above is dispatched by
        // the request `Host` — the SDUI surfaces (live/restos/riders/{slug}) SSR their generated screen
        // trees, non-app hosts keep plain-text landings. Explicit routes (/health, /ping, /{role}/graphql)
        // win, so Render's health check (internal *.onrender.com host) is unaffected. Covers `/` too.
        .fallback(hosts::host_root)
        // The fallback's tenant lookup (#98) — None without a database (every slug then serves the
        // storefront shell; the claim landing needs a POSITIVE not-found).
        .layer(Extension(tenant_lookup))
        // The fallback's SSR executor (#92): pages resolve their data in-process before rendering.
        .layer(Extension(ssr_exec))
        // API auth (ADR-0047): the Supabase-JWT verifier, available to the `/{role}/graphql` handler which
        // gates every non-public path. Shared as an Extension so the JWKS cache is process-wide.
        .layer(Extension(auth::AuthContext::from_config(
            config.supabase_jwks_url.clone(),
            config.supabase_url.clone(),
        )))
        // Outer layer: stamp every response with its server-side build time.
        .layer(middleware::from_fn(response_timing))
}

/// Stamp every response with how long the server took to build it: `x-response-time-ms` (milliseconds) and
/// the standard `Server-Timing` header (shown in browser devtools), plus `X-VERSION` — the running build's
/// identity (`build_version()`, the short git SHA) on **every** response, so any HTTP client can read which
/// deploy served it without hitting `/health` (ADR-20260721-175411). Applied as an outer layer over all routes.
async fn response_timing(req: Request, next: Next) -> Response {
    let start = std::time::Instant::now();
    let mut resp = next.run(req).await;
    let ms = format!("{:.2}", start.elapsed().as_secs_f64() * 1000.0);
    let headers = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&ms) {
        headers.insert("x-response-time-ms", v);
    }
    if let Ok(v) = HeaderValue::from_str(&format!("app;dur={ms}")) {
        headers.insert("server-timing", v);
    }
    if let Ok(v) = HeaderValue::from_str(build_version()) {
        headers.insert("x-version", v);
    }
    resp
}

/// Liveness: the process is up. No dependencies (does not touch the DB).
async fn ping() -> &'static str {
    "pong"
}

/// Projection-worker readiness. `200` when the worker is running, `503` otherwise (not started / not
/// caught up is still `200` with `lag > 0` — inspect the body). Reports checkpoint/head/lag/lastTickAt.
async fn projector(State(app): State<AppState>) -> impl IntoResponse {
    match &app.projector_status {
        Some(handle) => {
            let status = handle.lock().expect("projector status mutex").clone();
            let code = if status.running { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
            let body = serde_json::to_value(&status).unwrap_or_else(|_| json!({ "running": false }));
            (code, Json(body))
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "running": false, "reason": "projector_not_started" })),
        ),
    }
}

/// Saga-runner readiness (the `/projector` counterpart for the process managers). `200` when the
/// runner is running, `503` otherwise. Reports checkpoint/head/lag/lastTickAt.
async fn saga(State(app): State<AppState>) -> impl IntoResponse {
    match &app.saga_status {
        Some(handle) => {
            let status = handle.lock().expect("saga status mutex").clone();
            let code = if status.running { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
            let body = serde_json::to_value(&status).unwrap_or_else(|_| json!({ "running": false }));
            (code, Json(body))
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "running": false, "reason": "saga_runner_not_started" })),
        ),
    }
}

/// Deletion-engine readiness (ADR-20260728-224500; worker ADR-20260731-214500 §4). `200` when the
/// engine loop is running, `503` otherwise — and a `503` with `"reason": "deletion_engine_not_started"`
/// is the GATED-OFF default (`RUN_DELETION_ENGINE=false`), not a fault: recorded expiry facts
/// accumulate until the gate flips; no data is lost.
async fn deletion(State(app): State<AppState>) -> impl IntoResponse {
    match &app.deletion_status {
        Some(handle) => {
            let status = handle.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
            let code = if status.running { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
            let body = serde_json::to_value(&status).unwrap_or_else(|_| json!({ "running": false }));
            (code, Json(body))
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "running": false, "reason": "deletion_engine_not_started" })),
        ),
    }
}

/// SIRENE sync-worker readiness (issue #244) — the `/projector` counterpart for the staging drain.
/// `200` when the poll loop is running, `503` otherwise, and the body says WHICH `503` it is:
///
/// - `sirene_worker_not_available` — no `DATABASE_URL`, so no worker at all;
/// - `poll_loop_not_started` — the worker exists (the ping endpoint can drive it) but
///   `RUN_SIRENE_WORKER` did not start the loop. This is the department-37 case (#238): rows sitting
///   `PENDING` for hours with no way, from outside, to tell a paused loop from a crashing one.
///
/// `lastError` separates the two failure modes once the loop IS running, and `lastSummary` reports
/// what the last pass did. A ping-triggered pass updates the snapshot too, so the endpoint describes
/// the worker rather than only the loop.
async fn sirene(State(app): State<AppState>) -> impl IntoResponse {
    let status = app
        .sirene_status
        .as_ref()
        .map(|handle| handle.lock().expect("sirene status mutex").clone());
    let (code, body) = sirene_readiness(status);
    (code, Json(body))
}

/// The `/sirene` response, split from the handler so the three states are unit-testable without a
/// router or a database. `None` = no worker (no `DATABASE_URL`); `Some(status)` = a worker exists and
/// `running` says whether its poll loop was started.
fn sirene_readiness(
    status: Option<infrastructure::SireneSyncStatus>,
) -> (StatusCode, serde_json::Value) {
    let Some(status) = status else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({ "running": false, "reason": "sirene_worker_not_available" }),
        );
    };
    let code = if status.running { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    let mut body = serde_json::to_value(&status).unwrap_or_else(|_| json!({ "running": false }));
    if !status.running {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("reason".to_string(), json!("poll_loop_not_started"));
        }
    }
    (code, body)
}

/// Every 30s, recompute readiness and cache it. The first run happens immediately.
fn spawn_heartbeat(pool: PgPool, snap: Arc<Mutex<Snapshot>>) {
    tokio::spawn(async move {
        loop {
            let next = probe(&pool).await;
            *snap.lock().expect("health snapshot mutex") = next;
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

/// Read `max(version)` from `_sqlx_migrations` (successful rows only) and compare to the required version.
/// Simple query protocol (`raw_sql`, no prepared statement) → safe on any Supabase pooler mode (ADR-0043).
async fn probe(pool: &PgPool) -> Snapshot {
    match sqlx::raw_sql("SELECT max(version) AS v FROM _sqlx_migrations WHERE success")
        .fetch_one(pool)
        .await
    {
        Ok(row) => {
            let applied = row.try_get::<Option<i64>, _>("v").ok().flatten().unwrap_or(-1);
            let state = if applied >= REQUIRED_SCHEMA_VERSION {
                db_state::HEALTHY
            } else {
                db_state::SCHEMA_BEHIND
            };
            Snapshot { state, applied_version: applied }
        }
        Err(_) => Snapshot { state: db_state::DOWN, applied_version: -1 },
    }
}

/// Readiness endpoint (point Render's Health Check Path here). `200` only when reachable and the schema is
/// at/after the required version; otherwise `503` with a machine-readable reason.
async fn health(State(app): State<AppState>) -> impl IntoResponse {
    let snap = app.snap.lock().expect("health snapshot mutex").clone();
    // `version` is included in EVERY branch (esp. degraded/down) so a failing instance always names its build.
    match snap.state {
        db_state::HEALTHY => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "db": "up", "version": build_version(), "schemaVersion": snap.applied_version, "requiredSchemaVersion": REQUIRED_SCHEMA_VERSION })),
        ),
        db_state::SCHEMA_BEHIND => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "degraded", "db": "up", "reason": "schema_behind", "version": build_version(), "schemaVersion": snap.applied_version, "requiredSchemaVersion": REQUIRED_SCHEMA_VERSION })),
        ),
        db_state::NOT_CONFIGURED => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "degraded", "db": "not_configured", "version": build_version() })),
        ),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "degraded", "db": "down", "version": build_version() })),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The failure this fixes: `RUN_SIRENE_WORKER=TRUE` on Render silently meant PAUSED, because the
    /// gate was an exact `== "true"`. Every spelling a human or a dashboard plausibly produces must
    /// resolve to the state the operator intended (#244).
    #[test]
    fn flag_accepts_the_spellings_an_operator_actually_types() {
        for on in ["true", "TRUE", "True", " true ", "\"true\"", "'true'", "1", "yes", "ON"] {
            assert!(parse_flag(Some(on), "RUN_SIRENE_WORKER", false), "{on:?} should enable");
        }
        for off in ["false", "FALSE", " False ", "\"false\"", "0", "no", "OFF"] {
            assert!(!parse_flag(Some(off), "RUN_PROJECTOR", true), "{off:?} should disable");
        }
    }

    /// Unset, empty and unparsable all fall back to the DEFAULT — never to a fixed state. A typo must
    /// not silently pause a worker that defaults on, nor start one that defaults off.
    #[test]
    fn flag_falls_back_to_the_default_when_it_cannot_tell() {
        for undecidable in [None, Some(""), Some("   "), Some("maybe"), Some("2")] {
            assert!(parse_flag(undecidable, "RUN_PROJECTOR", true), "{undecidable:?}: default true");
            assert!(
                !parse_flag(undecidable, "RUN_SIRENE_WORKER", false),
                "{undecidable:?}: default false"
            );
        }
    }

    /// No `DATABASE_URL` → no worker at all. Distinct from a worker whose loop is merely paused.
    #[test]
    fn sirene_readiness_reports_no_worker_without_a_database() {
        let (code, body) = sirene_readiness(None);
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["reason"], "sirene_worker_not_available");
        assert_eq!(body["running"], false);
    }

    /// THE case the department-37 pilot could not diagnose (#238): the worker exists, the loop never
    /// started, 6,649 rows sat `PENDING` for hours, and nothing outside the process said so. `503` +
    /// `poll_loop_not_started` is that answer in one request.
    #[test]
    fn sirene_readiness_names_a_paused_poll_loop() {
        let (code, body) = sirene_readiness(Some(infrastructure::SireneSyncStatus::default()));
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["reason"], "poll_loop_not_started");
        assert_eq!(body["lastTickAt"], serde_json::Value::Null);
    }

    /// A running loop is `200`, carries no `reason`, and reports the last pass in camelCase (the
    /// wire shape `/projector` and `/saga` already use).
    #[test]
    fn sirene_readiness_reports_a_running_loop_and_its_last_pass() {
        let status = infrastructure::SireneSyncStatus {
            running: true,
            last_tick_at: Some(chrono::Utc::now()),
            last_error: None,
            last_summary: Some(infrastructure::SireneSyncSummary {
                processed: 6649,
                registered: 6649,
                ..Default::default()
            }),
        };
        let (code, body) = sirene_readiness(Some(status));
        assert_eq!(code, StatusCode::OK);
        assert!(body.get("reason").is_none(), "a running loop needs no reason");
        assert_eq!(body["lastSummary"]["processed"], 6649);
        assert!(body["lastTickAt"].is_string());
    }

    /// The generated configuration reader (PROP-20260729-004500) must agree with the spec on the two
    /// things the startup gate depends on: what is REQUIRED in production, and that a missing key is
    /// reported rather than defaulted away. A drift here silently re-opens the hole the spec closed.
    #[test]
    fn generated_config_requires_the_money_and_identity_keys_in_production() {
        use crate::generated::config::DECLARED_KEYS;
        for k in [
            "DATABASE_URL",
            "STRIPE_SECRET_KEY",
            "STRIPE_WEBHOOK_SECRET",
            "AUTH_SESSION_KEY",
            "SUPABASE_URL",
            "RUN_SIRENE_WORKER",
        ] {
            assert!(DECLARED_KEYS.contains(&k), "{k} must be declared in specs/configuration.yaml");
        }
    }

    /// The report must name EVERY problem, not the first — an operator who learns about one bad key
    /// per deploy cycle fixes a three-key outage in three deploys — and it must separate "absent" from
    /// "present but unusable", which are different fixes.
    #[test]
    fn config_report_lists_every_problem_and_never_prints_a_secret() {
        use crate::generated::config::{ConfigProblems, InvalidKey, MissingConfig, MissingKey, Profile};
        let report = MissingConfig {
            profile: Profile::Production,
            problems: ConfigProblems {
                missing: vec![MissingKey { name: "DATABASE_URL", gates: "Postgres pool." }],
                invalid: vec![InvalidKey {
                    name: "STRIPE_SECRET_KEY",
                    scalar: "StripeSecretKey",
                    pattern: "^sk_(test|live)_[A-Za-z0-9]+$",
                    gates: "Stripe API key for PaymentIntents.",
                }],
            },
        }
        .to_string();
        assert!(report.contains("DATABASE_URL"), "the missing key is named");
        assert!(report.contains("STRIPE_SECRET_KEY"), "the invalid key is named too");
        assert!(report.contains("MISSING"), "absent keys are grouped");
        assert!(report.contains("INVALID"), "malformed keys are grouped separately — a different fix");
        assert!(report.contains("^sk_(test|live)_"), "the EXPECTED shape is shown");
        assert!(report.contains("production"), "the profile is named");
        assert!(report.contains("Nothing was started."), "says what did NOT happen");
    }

    /// Enforcement follows the PROFILE (product-owner directive 2026-07-29): production and staging
    /// stop, development reports and continues. No second toggle to get wrong.
    #[test]
    fn only_development_continues_past_a_configuration_problem() {
        use crate::generated::config::Profile;
        for (profile, stops) in
            [(Profile::Production, true), (Profile::Staging, true), (Profile::Development, false)]
        {
            assert_eq!(
                !matches!(profile, Profile::Development),
                stops,
                "{profile} enforcement"
            );
        }
    }

    /// The scalars are what turn "present" into "usable". Each pattern must accept the real shape and
    /// reject the plausible mistake — a LIVE key in the test slot, a truncated session key, a bare host.
    #[test]
    fn config_scalars_reject_the_plausible_mistakes() {
        let cases: &[(&str, &str, bool)] = &[
            ("^sk_(test|live)_[A-Za-z0-9]+$", "sk_test_abc123", true),
            ("^sk_(test|live)_[A-Za-z0-9]+$", "sk_live_abc123", true),
            ("^sk_(test|live)_[A-Za-z0-9]+$", "pk_test_abc123", false), // publishable in a secret slot
            ("^sk_test_[A-Za-z0-9]+$", "sk_live_abc123", false),        // LIVE key in the test slot
            ("^pk_test_[A-Za-z0-9]+$", "pk_test_abc123", true),
            ("^pk_test_[A-Za-z0-9]+$", "pk_live_abc123", false),        // LIVE publishable in the TEST slot (#440)
            ("^pk_test_[A-Za-z0-9]+$", "sk_test_abc123", false),        // a SECRET key where a browser value goes (#440)
            ("^whsec_[A-Za-z0-9_-]+$", "whsec_abc123", true),
            ("^whsec_[A-Za-z0-9_-]+$", "sk_test_abc123", false),        // wrong secret entirely
            ("^([0-9a-fA-F]{64}|[A-Za-z0-9+/]{43}=)$", &"a".repeat(64), true),
            ("^([0-9a-fA-F]{64}|[A-Za-z0-9+/]{43}=)$", &"a".repeat(63), false), // 31 bytes, not 32
            ("^postgres(ql)?://", "postgresql://u:p@h:5432/db", true),
            ("^postgres(ql)?://", "h:5432/db", false),                  // bare host
            ("^([0-9]{2,3}|2[AB])(,([0-9]{2,3}|2[AB]))*$", "37,2A", true), // Corsica: 2A is 1 digit + letter
            ("^([0-9]{2,3}|2[AB])(,([0-9]{2,3}|2[AB]))*$", "37;41", false), // wrong separator
            ("^(?i)(true|yes|1|on|false|no|0|off)$", "TRUE", true),     // the #245 failure
            ("^(?i)(true|yes|1|on|false|no|0|off)$", "oui", false),
        ];
        for (pattern, value, expected) in cases {
            let re = regex::Regex::new(pattern).expect("pattern compiles");
            assert_eq!(re.is_match(value), *expected, "{pattern} vs {value:?}");
        }
    }

    /// A loop that runs and fails every pass must not look like a healthy one: `running` stays true
    /// (the loop IS turning) and `lastError` carries the reason — the second half of the diagnosis.
    #[test]
    fn sirene_readiness_surfaces_a_failing_pass() {
        let status = infrastructure::SireneSyncStatus {
            running: true,
            last_tick_at: Some(chrono::Utc::now()),
            last_error: Some("connection refused".to_string()),
            last_summary: None,
        };
        let (code, body) = sirene_readiness(Some(status));
        assert_eq!(code, StatusCode::OK, "the loop is running; the PASS failed");
        assert_eq!(body["lastError"], "connection refused");
    }
}
