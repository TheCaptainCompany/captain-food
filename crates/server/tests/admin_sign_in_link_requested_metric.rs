//! #639 part C step 6-iii round 2 R2-3 (obs B1 + reviewer B1) — `admin_sign_in_link_requested_total`
//! had ZERO production call sites, and the shared email send-abuse wall
//! (`crates/infrastructure/src/email_authorization.rs`) hardcoded the MEMBER counters regardless
//! of caller, so an admin's magic-link send silently landed on
//! `member_sign_in_link_requested_total`/`member_sign_in_refused_total` — a contract's own
//! population leaking into another's (ADR-20260905-223957 §5/§6).
//!
//! This proves, through the REAL dispatch seam (`infrastructure::inbox::route`, the same
//! `AdminSignIn` arm production traffic rides), that: (1) a successful `requestAdminSignInLink`
//! increments `admin_sign_in_link_requested_total{result=accepted}` and NEVER
//! `member_sign_in_link_requested_total`; (2) a refused one increments
//! `admin_sign_in_link_requested_total{result=refused}` and `admin_sign_in_refused_total`, never
//! the member counters — "the member path unchanged" (round 2 R2-3).
//!
//! Own test binary, the `otp_refusal_region_metric.rs` / `otp_guard_liveness_metric.rs` precedent:
//! `telemetry::meters` binds the process-wide OTel meter once via a `OnceLock`, and
//! `admin_sign_in_door.rs`'s nine tests already make live calls into
//! `telemetry::meters::admin_sign_in::*` on other threads — installing an `InMemoryMetricExporter`
//! as the GLOBAL provider inside that file would let concurrent calls land on this test's
//! exporter (or vice versa). A dedicated binary removes the race entirely.

use std::sync::{Arc, Mutex};

use application::generated::inboxes::ActorInbox;
use application::generated::services::{
    IdentityRefreshSessionInput, IdentityRefreshSessionOutput, IdentitySendAdminSignInLinkInput,
    IdentitySendEmailMagicLinkInput, IdentitySendPhoneOtpInput, IdentityService,
    IdentityStampAdminClaimInput, IdentityStampCustomerClaimInput, IdentityStampMemberClaimInput,
    IdentityStampRiderClaimInput, IdentityVerifyEmailTokenInput, IdentityVerifyEmailTokenOutput,
    IdentityVerifyPhoneOtpInput, IdentityVerifyPhoneOtpOutput, ServiceCallMeta,
};
use application::ports::{Actor, EventStore};
use async_trait::async_trait;
use domain::generated::scalars::EmailAddress;
use domain::shared::errors::DomainError;
use infrastructure::inbox::{route, CommandDeps, InboxOutcome, RouterEnv};
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedPaymentGateway, PgAuthSubjectReservationRepository,
    PgCatalogRepository, PgCustomerRepository, PgMemberRepository, PgPlatformMemberRepository,
    PgProspectionRepository, PgRestaurantRepository, PgRiderRepository, PgSlugReservationRepository,
    UnverifiedGbpOrderLinkProbe,
};
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};

/// This test never appends or loads anything — the request leg is a pure identity-port EFFECT.
struct UntouchableEventStore;
#[async_trait]
impl EventStore for UntouchableEventStore {
    async fn append(&self, stream_name: &str, _expected_version: i64, _events: &[domain::generated::events::DomainEvent], _actor: &Actor) -> Result<i64, DomainError> {
        panic!("requestAdminSignInLink must append NOTHING (stream {stream_name})");
    }
    async fn load(&self, stream_name: &str) -> Result<(Vec<domain::generated::events::DomainEvent>, i64), DomainError> {
        panic!("requestAdminSignInLink must load NO stream (stream {stream_name})");
    }
}

/// An `IdentityService` that answers ONLY `send_admin_sign_in_link` (the ONE call
/// `request_admin_sign_in_link` makes) with a scripted `Ok`/`Err`; every other method panics —
/// this test's whole point is that NOTHING else on the identity port is reachable from this leg.
struct ScriptedAdminSend {
    outcome: Mutex<Vec<Result<(), &'static str>>>,
    sent: Mutex<Vec<String>>,
}

impl ScriptedAdminSend {
    fn accepting() -> Self {
        Self { outcome: Mutex::new(vec![Ok(())]), sent: Mutex::new(Vec::new()) }
    }
    fn refusing(reason: &'static str) -> Self {
        Self { outcome: Mutex::new(vec![Err(reason)]), sent: Mutex::new(Vec::new()) }
    }
}

#[async_trait]
impl IdentityService for ScriptedAdminSend {
    async fn send_admin_sign_in_link(&self, input: IdentitySendAdminSignInLinkInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        self.sent.lock().expect("scripted admin send").push(input.email.0);
        match self.outcome.lock().expect("scripted admin send").remove(0) {
            Ok(()) => Ok(()),
            Err(code) => Err(DomainError::Rejected { code: code.into(), context: serde_json::json!({}) }),
        }
    }
    async fn send_email_magic_link(&self, _input: IdentitySendEmailMagicLinkInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("the admin request leg must never call the shared member/customer send_email_magic_link")
    }
    async fn send_phone_otp(&self, _input: IdentitySendPhoneOtpInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("unreached by requestAdminSignInLink")
    }
    async fn verify_phone_otp(&self, _input: IdentityVerifyPhoneOtpInput, _meta: &ServiceCallMeta) -> Result<IdentityVerifyPhoneOtpOutput, DomainError> {
        panic!("unreached by requestAdminSignInLink")
    }
    async fn refresh_session(&self, _input: IdentityRefreshSessionInput, _meta: &ServiceCallMeta) -> Result<IdentityRefreshSessionOutput, DomainError> {
        panic!("unreached by requestAdminSignInLink")
    }
    async fn stamp_customer_claim(&self, _input: IdentityStampCustomerClaimInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("unreached by requestAdminSignInLink")
    }
    async fn stamp_rider_claim(&self, _input: IdentityStampRiderClaimInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("unreached by requestAdminSignInLink")
    }
    async fn verify_email_token(&self, _input: IdentityVerifyEmailTokenInput, _meta: &ServiceCallMeta) -> Result<IdentityVerifyEmailTokenOutput, DomainError> {
        panic!("unreached by requestAdminSignInLink")
    }
    async fn stamp_member_claim(&self, _input: IdentityStampMemberClaimInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("unreached by requestAdminSignInLink")
    }
    async fn stamp_admin_claim(&self, _input: IdentityStampAdminClaimInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("unreached by requestAdminSignInLink")
    }
}

fn deps(auth: Arc<dyn IdentityService>) -> CommandDeps {
    // A lazily-connected pool never actually dialed (`connect_lazy`): every repository below is
    // wired to it exactly like `admin_sign_in_door.rs`'s own fixture, and NONE of them is queried
    // on the request leg (`request_admin_sign_in_link` touches only `auth`).
    let unused: sqlx::PgPool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .expect("a lazy pool connects to nothing");
    CommandDeps {
        store: Arc::new(UntouchableEventStore),
        restaurants: Arc::new(PgRestaurantRepository::new(unused.clone())),
        slugs: Arc::new(PgSlugReservationRepository::new(unused.clone())),
        auth_subjects: Arc::new(PgAuthSubjectReservationRepository::new(unused.clone())),
        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(PgProspectionRepository::new(unused.clone())),
        catalogs: Arc::new(PgCatalogRepository::new(unused.clone())),
        auth,
        customers: Arc::new(PgCustomerRepository::new(unused.clone())),
        sessions: Arc::new(application::auth_sessions::mem::MemAuthSessionStore::default()),
        payments: Arc::new(FailClosedPaymentGateway),
        pm_state: Arc::new(application::generated::pm_state::mem::MemPaymentProcessState::default()),
        refund_state: Arc::new(application::generated::pm_state::mem::MemRefundProcessState::default()),
        mailbox_requeue: Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(unused.clone())),
        enforce_service_hours_guard: false,
        enforce_acceptance_timeout: false,
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: true,
            place_replacement_order_to_order: false,
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        },
        riders: Arc::new(PgRiderRepository::new(unused.clone())),
        members: Arc::new(PgMemberRepository::new(unused.clone())),
        support_contact: Some(EmailAddress("support@captain.food".into())),
        run_rider_restriction_door: false,
        run_member_access_grant: false,
        run_member_sign_in_door: false,
        run_restaurant_invitation: false,
        run_platform_access_grant: false,
        platform_members: Arc::new(PgPlatformMemberRepository::new(unused.clone())),
        run_admin_sign_in_door: true,
    }
}

async fn request(deps: &CommandDeps, email: &str) -> InboxOutcome {
    let cmd = domain::generated::commands::RequestAdminSignInLink {
        email: EmailAddress(email.into()),
        locale: None,
    };
    let inbox = ActorInbox::AdminSignIn(
        application::generated::inboxes::AdminSignInInbox::RequestAdminSignInLink(cmd),
    );
    let actor = Actor {
        user_id: uuid::Uuid::nil(),
        user_type: "PUBLIC".into(),
        domain_id: None,
        correlation_id: uuid::Uuid::now_v7(),
        cause_id: None,
    };
    route(deps, inbox, &actor, &RouterEnv { session_id: None }).await
}

/// Every `{name}` data point collected so far, as `(metric_name, result_or_reason)` pairs.
/// Cumulative temporality means later flushes still carry earlier points.
fn points(exporter: &InMemoryMetricExporter, wanted: &str, label: &str) -> Vec<String> {
    let mut out = Vec::new();
    // Cumulative temporality: EVERY flush since the provider was installed is a separate export
    // in this list, and each later export ALREADY carries the running total for every attribute
    // combination seen so far -- only the LAST export is the current cumulative truth (the
    // `otp_refusal_region_metric.rs` precedent instead de-dupes distinct VALUES across exports;
    // here the whole point is the exact COUNT per result/reason, so only the latest is read).
    let Some(rm) = exporter.get_finished_metrics().expect("finished metrics").into_iter().last() else {
        return out;
    };
    for scope in rm.scope_metrics() {
        for metric in scope.metrics() {
            if metric.name() != wanted {
                continue;
            }
            let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() else {
                panic!("{wanted} must aggregate as a u64 Sum: {:?}", metric.data());
            };
            for dp in sum.data_points() {
                let value = dp
                    .attributes()
                    .find(|kv| kv.key.as_str() == label)
                    .map(|kv| kv.value.to_string())
                    .unwrap_or_default();
                for _ in 0..dp.value() {
                    out.push(value.clone());
                }
            }
        }
    }
    out.sort();
    out
}

#[tokio::test]
async fn a_requested_admin_link_counts_on_the_admin_contract_and_never_the_member_one() {
    let exporter = InMemoryMetricExporter::default();
    let provider = SdkMeterProvider::builder().with_periodic_exporter(exporter.clone()).build();
    opentelemetry::global::set_meter_provider(provider.clone());

    // PHASE 1 -- ACCEPTED: red before round 2 R2-3 (the emitter did not exist at all, and the
    // shared wall hardcoded MEMBER counters even when it did land somewhere) -- green now.
    let accepted = deps(Arc::new(ScriptedAdminSend::accepting()));
    let outcome = request(&accepted, "admin@captain.food").await;
    assert!(matches!(outcome, InboxOutcome::Handled(Ok(()))), "the scripted send must accept");
    provider.force_flush().expect("flush after the accepted request");
    assert_eq!(
        points(&exporter, "admin_sign_in_link_requested_total", "result"),
        vec!["accepted".to_string()],
        "an accepted requestAdminSignInLink must count on admin_sign_in_link_requested_total{{result=accepted}}"
    );
    assert!(
        points(&exporter, "member_sign_in_link_requested_total", "result").is_empty(),
        "an ADMIN send must NEVER move the MEMBER door's counter -- round 2 R2-3's whole point"
    );

    // PHASE 2 -- REFUSED (the shared wall's own rate limit, or any other typed rejection): counts
    // on BOTH admin_sign_in_link_requested_total{result=refused} and admin_sign_in_refused_total,
    // still never touching the member counters ("the member path unchanged").
    let refused = deps(Arc::new(ScriptedAdminSend::refusing("RateLimited")));
    let outcome = request(&refused, "stranger@example.com").await;
    assert!(matches!(outcome, InboxOutcome::Handled(Err(_))), "the scripted send must refuse");
    provider.force_flush().expect("flush after the refused request");
    assert_eq!(
        points(&exporter, "admin_sign_in_link_requested_total", "result"),
        vec!["accepted".to_string(), "refused".to_string()],
        "the refusal must ALSO count on admin_sign_in_link_requested_total{{result=refused}}"
    );
    assert_eq!(
        points(&exporter, "admin_sign_in_refused_total", "reason"),
        vec!["rate_limited".to_string()],
        "the typed refusal reason must land on admin_sign_in_refused_total"
    );
    assert!(
        points(&exporter, "member_sign_in_link_requested_total", "result").is_empty(),
        "still never the MEMBER door's counter"
    );
    assert!(
        points(&exporter, "member_sign_in_refused_total", "reason").is_empty(),
        "still never the MEMBER door's refused counter"
    );
}
