//! Integration test for the ADR-0045 staging→worker slice, post-ADR-20260731-122500 ("the mailbox
//! is the only door"): a raw INSEE row in `external_sirene_restaurants` → `SireneSyncWorker::run_once`
//! → ACL → a kind-EVENT row on the MAILBOX (INSEE cannot be told "no", so registrations are recorded,
//! not requested) → the MailboxWorker delivers it through the fenced write path and the AGGREGATE
//! decides → the verdict is reconciled back onto the mirror (`STAGED` → `SYNCED`/`FAILED`, #231).
//! The explicit `etat=F` closure stays a COMMAND (`MarkRestaurantClosed` — our inference, refusable)
//! but is now a fire-and-forget WORKER-channel enqueue: `closed` counts HAND-OFFS, and the event
//! lands when the mailbox worker delivers.
//! Needs a real Postgres: set `DATABASE_URL` (see restaurant_write_path.rs for a throwaway docker
//! one-liner). Without it the test SKIPS so `cargo test` stays green offline.

use std::sync::Arc;

use application::ports::{Actor, EventStore};
use domain::generated::entities::Address;
use domain::generated::events::{DomainEvent, RestaurantRegistered};
use domain::generated::scalars::{
    AddressLine, CityName, CountryCode, PostalCode, RestaurantDisplayName, RestaurantId,
    RestaurantListingStatus,
};
use actor_runtime::{MailboxWorker, WorkerConfig};
use application::generated::services::{IdentityService, PaymentService};
use infrastructure::generated::command_router::CommandDeps;
use infrastructure::integrations::sirene::restaurant_id_for_siret;
use infrastructure::mailbox::MailboxCommandHandler;
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCatalogRepository, PgCustomerRepository, PgEventStore, PgProspectionRepository,
    PgRestaurantRepository, PgAuthSubjectReservationRepository, PgSlugReservationRepository, SireneSyncWorker,
    UnverifiedGbpOrderLinkProbe,
};
use sqlx::PgPool;

/// The same realistic Sirene 3.11 shape the ACL/ingestion tests use, with a parameterizable état.
fn sample_payload(etat: &str) -> serde_json::Value {
    serde_json::json!({
        "siren": "852421099",
        "nic": "00021",
        "siret": "85242109900021",
        "uniteLegale": { "denominationUniteLegale": "SARL CHEZ MARCO",
                         "activitePrincipaleUniteLegale": "56.10A" },
        "adresseEtablissement": {
            "numeroVoieEtablissement": "12",
            "typeVoieEtablissement": "RUE",
            "libelleVoieEtablissement": "NATIONALE",
            "codePostalEtablissement": "37000",
            "libelleCommuneEtablissement": "TOURS",
            "codeCommuneEtablissement": "37261"
        },
        "periodesEtablissement": [ {
            "dateFin": null,
            "etatAdministratifEtablissement": etat,
            "enseigne1Etablissement": "CHEZ MARCO",
            "activitePrincipaleEtablissement": "56.10A"
        } ]
    })
}

/// Stage one row the way the ingestion does (fresh `last_seen_at`, untouched `processed_at`, and
/// a content hash — the real migrated column is NOT NULL with its default deliberately dropped,
/// so a hashless insert is unspellable in production and must be here too).
async fn stage_row(pool: &PgPool, etat: &str) {
    sqlx::query(
        "INSERT INTO external_sirene_restaurants \
           (siret, payload, etat, naf, department, first_seen_at, last_seen_at, sync_run_id, payload_hash, processed_at) \
         VALUES ('85242109900021', $1, $2, '56.10A', '37', now(), now(), $3, md5($1::text), NULL) \
         ON CONFLICT (siret) DO UPDATE SET \
           payload = EXCLUDED.payload, etat = EXCLUDED.etat, last_seen_at = EXCLUDED.last_seen_at, \
           sync_run_id = EXCLUDED.sync_run_id, payload_hash = EXCLUDED.payload_hash",
    )
    .bind(sample_payload(etat))
    .bind(etat)
    .bind(uuid::Uuid::new_v4())
    .execute(pool)
    .await
    .expect("stage row");
}

/// Stage a row carrying an arbitrary payload (the `stage_row` helper always stages a mappable one).
async fn stage_row_with_payload(pool: &PgPool, siret: &str, payload: serde_json::Value) {
    sqlx::query(
        "INSERT INTO external_sirene_restaurants \
           (siret, payload, etat, naf, department, first_seen_at, last_seen_at, sync_run_id, payload_hash, processed_at) \
         VALUES ($1, $2, 'A', '56.10A', '37', now(), now(), $3, md5($2::text), NULL) \
         ON CONFLICT (siret) DO UPDATE SET \
           payload = EXCLUDED.payload, last_seen_at = EXCLUDED.last_seen_at, \
           sync_run_id = EXCLUDED.sync_run_id, payload_hash = EXCLUDED.payload_hash",
    )
    .bind(siret)
    .bind(payload)
    .bind(uuid::Uuid::new_v4())
    .execute(pool)
    .await
    .expect("stage row with payload");
}

/// The payload lifetime (#231, ADR-20260728-143000): the worker DROPS the payload it has translated and
/// KEEPS the one it could not map.
///
/// Both halves matter. Dropping the translated payload is the 655 MB — the mirror was 77% of the
/// database because it kept verbatim records forever to read five fields out of them. Keeping the
/// unmappable one is the diagnostic: that payload is the only evidence of WHY INSEE's record was
/// unusable, and a silent unmappable row with no evidence is how a systematic mapping bug hides.
#[tokio::test]
async fn worker_drops_the_payload_it_translated_and_keeps_what_it_could_not_map() {
    let Some(db) = crate::common::TestDb::acquire("sync_sirene_worker").await else { return };
    let pool = db.pool();
    let worker = SireneSyncWorker::new(pool.clone());

    // A mappable record, and one that parses as an établissement but that the ACL rejects (no
    // enseigne/denomination anywhere, so there is no usable name).
    stage_row(&pool, "A").await;
    stage_row_with_payload(
        &pool,
        "85242109900039",
        serde_json::json!({
            "siret": "85242109900039",
            "adresseEtablissement": { "codePostalEtablissement": "37000",
                                      "libelleCommuneEtablissement": "TOURS" },
            "periodesEtablissement": [ { "dateFin": null,
                                         "etatAdministratifEtablissement": "A",
                                         "activitePrincipaleEtablissement": "56.10A" } ]
        }),
    )
    .await;

    let summary = worker.run_once().await.expect("drain");
    assert_eq!(summary.processed, 2, "both rows drained");
    assert_eq!(summary.registered, 1, "one staged as an inbound registry fact");
    assert_eq!(summary.skipped, 1, "one could not be mapped");

    // The snapshot `/sirene` serves (#244). A ping-triggered pass records itself too — the endpoint
    // describes the WORKER, not just the poll loop — while `running` stays false here because no loop
    // was started. That pair is the distinction the department-37 pilot (#238) had no way to make.
    {
        let handle = worker.status();
        let snapshot = handle.lock().expect("sirene status");
        assert!(!snapshot.running, "this test never started the poll loop");
        assert!(snapshot.last_tick_at.is_some(), "the pass stamped itself");
        assert!(snapshot.last_error.is_none(), "a successful pass leaves no error");
        assert_eq!(
            snapshot.last_summary.as_ref().map(|s| s.registered),
            Some(1),
            "the snapshot carries the pass's own counters"
        );
    }

    let (payload, hash, status): (Option<serde_json::Value>, String, String) = sqlx::query_as(
        "SELECT payload, payload_hash, status FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("translated row");
    // STAGED, not SYNCED: the fact has been handed to the inbox but the AGGREGATE has not decided yet.
    // Claiming SYNCED here would assert a success this worker never observed — and the payload must
    // survive with it, because an unaccepted translation is exactly when the raw record is still needed.
    assert_eq!(status, "STAGED", "handed over — the verdict is not in yet");
    assert!(payload.is_some(), "nothing is confirmed, so the only original is not thrown away");
    assert!(!hash.is_empty(), "the hash persists: it is what stops the next sweep re-pending the row");

    let (unmappable, unmappable_status): (Option<serde_json::Value>, String) = sqlx::query_as(
        "SELECT payload, status FROM external_sirene_restaurants WHERE siret = '85242109900039'",
    )
    .fetch_one(&pool)
    .await
    .expect("unmappable row");
    assert!(unmappable.is_some(), "the evidence of an unusable INSEE record is kept");
    // Both rows now hold different things and say why. Without `status` these two would be
    // "payload NULL" vs "payload present" with no way to tell evidence from a pending translation.
    assert_eq!(unmappable_status, "UNMAPPABLE", "and it is NOT reported as synced");

    // Both rows are checkpointed either way — an unmappable row must not be retried forever.
    let still_pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM external_sirene_restaurants \
          WHERE processed_at IS NULL OR processed_at < last_seen_at",
    )
    .fetch_one(&pool)
    .await
    .expect("pending count");
    assert_eq!(still_pending, 0, "keeping a payload is not the same as leaving the row pending");
}

/// The real delivery half (ADR-20260731-122500), wired over the same pool — the production
/// MailboxWorker + MailboxCommandHandler, so these tests assert what production does with an
/// enqueued row, not a hand-simulated verdict. Returns the delivered count for the pass.
async fn deliver_once(pool: &PgPool) -> u64 {
    let deps = CommandDeps {
        // #639 part C step 2c-i: the rider sign-in door's bridge + support route (not exercised here).
        riders: Arc::new(infrastructure::PgRiderRepository::new(pool.clone())),
        members: Arc::new(infrastructure::PgMemberRepository::new(pool.clone())),
        support_contact: None,
        run_rider_restriction_door: false,
        run_member_access_grant: false,
        run_member_sign_in_door: false,
        store: Arc::new(PgEventStore::new(pool.clone())),
        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(PgSlugReservationRepository::new(pool.clone())),
        auth_subjects: Arc::new(PgAuthSubjectReservationRepository::new(pool.clone())),
        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(PgCatalogRepository::new(pool.clone())),
        auth: Arc::new(FailClosedIdentityService) as Arc<dyn IdentityService>,
        customers: Arc::new(PgCustomerRepository::new(pool.clone())),
        sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
        payments: Arc::new(FailClosedPaymentGateway) as Arc<dyn PaymentService>,
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(pool.clone())),
        mailbox_requeue: Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(pool.clone())),
        // RSO-1: the service-hours enforcement gate at its spec default (OFF = shadow).
        enforce_service_hours_guard: false,
        // #167: the acceptance-timeout gate at its spec default (OFF = shadow).
        enforce_acceptance_timeout: false,
        // #588: the Order-lane birth routing at its spec default (OFF = the legacy append).
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: false,
            place_replacement_order_to_order: false,
            // #807: routed `send:` steps -- OFF, this fixture exercises the birth routes.
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        },
    };
    let worker = MailboxWorker::new(
        pool.clone(),
        "w-test",
        "Restaurant",
        // Zero-length lease: each deliver_once call is a fresh takeover — the previous call's
        // lanes are instantly claimable again (a test calls this several times per scenario).
        WorkerConfig { lease_seconds: 0, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps)),
    );
    worker.seed(5).await.expect("seed");
    worker.claim().await.expect("claim");
    worker.drain().await.expect("drain")
}

#[tokio::test]
async fn worker_drains_staging_rows_through_the_write_path_idempotently_and_closes_prospects() {
    let Some(db) = crate::common::TestDb::acquire("sync_sirene_worker").await else { return };
    let pool = db.pool();
    let restaurant_id = restaurant_id_for_siret("85242109900021").0;
    let worker = SireneSyncWorker::new(pool.clone());

    // 1) A pending staged row is handed over as ONE inbound registry FACT — and nothing more. No
    //    command (INSEE cannot be told "no", ADR-20260728-011344 D4), and no domain event yet: the
    //    hand-over is not the delivery, and the aggregate has not decided anything.
    stage_row(&pool, "A").await;
    let summary = worker.run_once().await.expect("first drain");
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.registered, 1, "one registry fact handed to the inbox");
    assert_eq!(summary.failed, 0);

    let (mailbox_message_id, source, external_id, event_type, inbound_status): (
        uuid::Uuid,
        String,
        String,
        String,
        String,
    ) = sqlx::query_as(
        "SELECT message_id, source, external_id, message_type, status FROM inbound_messages",
    )
    .fetch_one(&pool)
    .await
    .expect("one inbound fact");
    assert_eq!(source, "sirene");
    // The stable dedupe key: the staged version is identified by WHAT the record says
    // (`{siret}:{payload_hash}`), not by WHEN it was seen — which is what lets a later sweep
    // re-see the same content without re-staging it.
    let staged_hash: String = sqlx::query_scalar(
        "SELECT payload_hash FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("staged hash");
    assert_eq!(external_id, format!("85242109900021:{staged_hash}"));
    assert_eq!(event_type, "RestaurantRegistered");
    assert_eq!(inbound_status, "RECEIVED", "RECEIVED — awaiting the mailbox worker");

    let register_commands: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM inbound_messages WHERE kind = 'COMMAND' AND message_type = 'RegisterRestaurant'",
    )
    .fetch_one(&pool)
    .await
    .expect("count register command rows");
    assert_eq!(register_commands, 0, "a fact is recorded, not requested — no command is enqueued");
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("count events");
    assert_eq!(events, 0, "hand-over is not delivery — nothing on the log yet");

    let pending: bool = sqlx::query_scalar(
        "SELECT processed_at IS NULL OR processed_at < last_seen_at \
         FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("pending flag");
    assert!(!pending, "a drained row must carry its processed_at checkpoint");

    // 2) The REAL mailbox worker delivers the fact through the fenced write path: ONE
    //    RestaurantRegistered on the deterministic UUIDv5(SIRET) stream, stamped EXTERNAL, and
    //    caused by the exact mailbox row that carried it.
    assert_eq!(deliver_once(&pool).await, 1);

    let (stream, event_type, user_type, cause_id, payload): (
        String,
        String,
        String,
        Option<uuid::Uuid>,
        serde_json::Value,
    ) = sqlx::query_as(
        "SELECT stream_name, event_type, user_type, cause_id, payload FROM domain_events",
    )
    .fetch_one(&pool)
    .await
    .expect("one event row");
    assert_eq!(stream, format!("Restaurant-{restaurant_id}"));
    assert_eq!(event_type, "RestaurantRegistered");
    assert_eq!(user_type, "EXTERNAL"); // EXTERNAL envelope stamp (ADR-0041)
    assert_eq!(cause_id, Some(mailbox_message_id), "the fact chains to the mailbox row");
    assert_eq!(payload["ref"], serde_json::json!("85242109900021"));
    assert_eq!(payload["listingStatus"], serde_json::json!("NON_PARTNER"));

    // 3) The aggregate's verdict is reconciled onto the mirror by the next pass — which is otherwise
    //    a complete no-op — and only THERE does the row become SYNCED and lose its payload (#231).
    let replay = worker.run_once().await.expect("reconcile pass");
    assert_eq!(replay.processed, 0, "nothing pending — an idempotent re-run drains nothing");
    assert_eq!(replay.resolved, 1, "the DELIVERED verdict lands on the mirror");
    let (mirror_status, synced_at, mirror_payload): (
        String,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<serde_json::Value>,
    ) = sqlx::query_as(
        "SELECT status, synced_at, payload FROM external_sirene_restaurants \
          WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("reconciled row");
    assert_eq!(mirror_status, "SYNCED");
    assert!(synced_at.is_some(), "the sync's completion is timestamped");
    assert!(mirror_payload.is_none(), "confirmed — the payload is finally released");

    // 4) A re-ingested row (same SIRET, same content, refreshed last_seen_at) is pending again, but
    //    the stable (source, external_id) key absorbs it: nothing new staged, nothing appended, and
    //    the already-recorded verdict re-confirms the row within the same pass.
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    stage_row(&pool, "A").await;
    let refresh = worker.run_once().await.expect("refresh drain");
    assert_eq!(refresh.processed, 1);
    assert_eq!(refresh.registered, 0, "the same fact is not re-staged");
    assert_eq!(refresh.skipped, 1, "it deduplicates on the inbox instead");
    assert_eq!(refresh.resolved, 1, "and the known verdict re-confirms the row in the same pass");
    let inbound_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inbound_messages WHERE kind = 'EVENT'")
        .fetch_one(&pool)
        .await
        .expect("count inbound rows");
    assert_eq!(inbound_rows, 1, "one fact, however many times the sweep re-sees it");
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("count events after replay");
    assert_eq!(events, 1);

    // 5) Deletion reconciliation (ADR-0045): an explicit etat=F refresh closes the NON_PARTNER
    //    prospect via the ordinary MarkRestaurantClosed handler. Closure IS still a command — the
    //    closure is our inference and can be refused — so this half journals (WORKER channel, #15)…
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    stage_row(&pool, "F").await;
    let closing = worker.run_once().await.expect("closing drain");
    assert_eq!(closing.processed, 1);
    assert_eq!(closing.closed, 1, "the close signal is HANDED OFF (fire-and-forget)");
    // The hand-off is not the delivery: the event lands when the mailbox worker runs.
    assert_eq!(deliver_once(&pool).await, 1);
    let (last_type,): (String,) = sqlx::query_as(
        "SELECT event_type FROM domain_events ORDER BY position DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("latest event");
    assert_eq!(last_type, "RestaurantMarkedClosed");

    // …and repeating the signal is absorbed (the aggregate already folds to INACTIVE).
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    stage_row(&pool, "F").await;
    let closed_again = worker.run_once().await.expect("idempotent closing drain");
    assert_eq!(closed_again.closed, 0);
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("final event count");
    assert_eq!(events, 2, "register + one close, no matter how often the signal repeats");
    // The repeated signal never reached the enqueue (the aggregate already folds INACTIVE), so the
    // mailbox holds exactly ONE MarkRestaurantClosed submission — WORKER channel, SUCCEEDED.
    let (close_rows, close_channel_status): (i64, i64) = (
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM inbound_messages WHERE message_type = 'MarkRestaurantClosed'",
        )
        .fetch_one(&pool)
        .await
        .expect("count close mailbox rows"),
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM inbound_messages \
             WHERE message_type = 'MarkRestaurantClosed' AND channel = 'WORKER' AND status = 'SUCCEEDED'",
        )
        .fetch_one(&pool)
        .await
        .expect("count close mailbox rows by channel/status"),
    );
    assert_eq!((close_rows, close_channel_status), (1, 1));
}

/// Seed the `restaurant` projection the way production looks for pre-derivation listings: a row
/// owning the slug under an arbitrary legacy aggregate id, carrying (or not) the SIRET identifier.
async fn seed_projection_row(pool: &PgPool, id: uuid::Uuid, slug: &str, identifiers: serde_json::Value) {
    sqlx::query(
        "INSERT INTO restaurant (restaurant_id, listing_status, external_identifiers, slug, \
           display_name, address, opening_hours, status, order_acceptance, default_currency, \
           created_at, updated_at) \
         VALUES ($1, 'NON_PARTNER', $2, $3, 'CHEZ MARCO', '{}'::jsonb, '[]'::jsonb, 'DRAFT', 'NORMAL', 'EUR', now(), now())",
    )
    .bind(id)
    .bind(identifiers)
    .bind(slug)
    .execute(pool)
    .await
    .expect("seed projection row");
}

/// Production predates the UUIDv5(SIRET) derivation, so legacy listings live under other aggregate
/// ids and the projection row carrying the SIRET is the only thing that NAMES them. Since #227 the
/// adoption survives on the CLOSE path only: the register path derives its id deterministically and
/// never asks the read model, but a closure aimed at the derived id would close a sibling that does
/// not exist — leaving the real, legacy-id listing open and orderable, which is the worse failure.
#[tokio::test]
async fn worker_adopts_the_legacy_aggregate_id_the_projection_names_for_a_known_siret() {
    let Some(db) = crate::common::TestDb::acquire("sync_sirene_worker").await else { return };
    let pool = db.pool();
    let legacy_id = uuid::Uuid::new_v4();
    assert_ne!(legacy_id, restaurant_id_for_siret("85242109900021").0);

    // The legacy aggregate EXISTS in the log under its pre-derivation id, and the projection row
    // carrying the SIRET names it — exactly the shape production inherited.
    let store = PgEventStore::new(pool.clone());
    let registered = DomainEvent::RestaurantRegistered(RestaurantRegistered {
        mode: None,
        restaurant_id: RestaurantId(legacy_id),
        account_id: None,
        listing_status: RestaurantListingStatus::NON_PARTNER,
        r#ref: None,
        external_identifiers: vec![],
        display_name: RestaurantDisplayName("CHEZ MARCO".into()),
        contact: None,
        website: None,
        tags: vec![],
        margin_rate: None,
        cuisine_category: None,
        uber_prices_opt_in: None,
        address: Address {
            line1: AddressLine("12 RUE NATIONALE".into()),
            line2: None,
            postal_code: PostalCode("37000".into()),
            city: CityName("TOURS".into()),
            country: CountryCode("FR".into()),
        },
        location: None,
        timezone: None,
        preparation_time_minutes: None,
        opening_hours: vec![],
    });
    let actor = Actor {
        user_id: uuid::Uuid::nil(),
        user_type: "EXTERNAL".to_string(),
        domain_id: None,
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    };
    store
        .append(&format!("Restaurant-{legacy_id}"), 0, std::slice::from_ref(&registered), &actor)
        .await
        .expect("seed the legacy aggregate");
    seed_projection_row(
        &pool,
        legacy_id,
        "chez-marco-00021",
        serde_json::json!([{ "key": "siret", "value": "85242109900021" }]),
    )
    .await;
    let worker = SireneSyncWorker::new(pool.clone());

    // An explicit closure signal for that SIRET must close the aggregate the projection NAMES,
    // not a derived sibling.
    stage_row(&pool, "F").await;
    let closing = worker.run_once().await.expect("closing drain");
    assert_eq!(closing.closed, 1, "handed off");
    assert_eq!(closing.failed, 0);
    assert_eq!(deliver_once(&pool).await, 1, "the mailbox worker delivers the close");
    let (stream, event_type): (String, String) = sqlx::query_as(
        "SELECT stream_name, event_type FROM domain_events ORDER BY position DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("close event");
    assert_eq!(
        (stream.as_str(), event_type.as_str()),
        (format!("Restaurant-{legacy_id}").as_str(), "RestaurantMarkedClosed")
    );
    // And the register-path contrast (#227): a closure needs no inbound FACT — kind COMMAND only.
    let inbound_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inbound_messages WHERE kind = 'EVENT'")
        .fetch_one(&pool)
        .await
        .expect("count inbound rows");
    assert_eq!(inbound_rows, 0, "closing is a command, not an inbound fact");
}

/// The modern shape of the retired "deterministically rejected row" test. Since #227 the register
/// path has no rejections — a slug is not even validated (the worker stages a FACT; a prospect has no
/// slug until one is configured). What remains deterministic is a DELIVERY failure: a staged fact the
/// write path cannot apply. That verdict must (a) leave a durable, queryable trace instead of an
/// eprintln nobody reads, (b) reconcile onto the mirror as FAILED with the payload KEPT (it is the
/// only original if re-translation is needed), and (c) not be retried every pass — the production
/// 605-row SlugAlreadyTaken log storm was exactly the retry-forever shape this guards against.
///
/// **What "durable trace" means changed with #780** and the assertion below moved with it: the
/// door's undecodable-payload verdict used to record `context.detail: <free text>`, one of the
/// legacy sites the #623 leak canary names, and now records the bounded
/// `CommandFailureAttribution` pair. The row still says WHY — more precisely, in a vocabulary
/// support can act on — and the diagnostic prose lives in the log, where nothing keeps it for
/// ninety days.
/// Recovery needs no operator action: a changed record from INSEE re-pends the row.
#[tokio::test]
async fn a_failed_delivery_leaves_a_durable_trace_and_is_not_retried_forever() {
    let Some(db) = crate::common::TestDb::acquire("sync_sirene_worker").await else { return };
    let pool = db.pool();
    let worker = SireneSyncWorker::new(pool.clone());

    stage_row(&pool, "A").await;
    let summary = worker.run_once().await.expect("stage the fact");
    assert_eq!(summary.registered, 1);

    // Break the staged fact the way an ACL/schema drift would: the payload no longer parses as a
    // DomainEvent. The REAL drain worker then records the verdict — this is not simulated.
    sqlx::query(
        "UPDATE inbound_messages SET payload = '{\"eventType\":\"NotAnEvent\"}'::jsonb \
          WHERE source = 'sirene'",
    )
    .execute(&pool)
    .await
    .expect("corrupt the staged fact");
    // The mailbox worker delivers the row and records the FAILED verdict — not simulated.
    assert_eq!(deliver_once(&pool).await, 1);

    // (a) The verdict is durable and queryable — support can answer "what happened to this row".
    let (inbound_status, error): (String, Option<serde_json::Value>) =
        sqlx::query_as("SELECT status, error FROM inbound_messages WHERE source = 'sirene'")
            .fetch_one(&pool)
            .await
            .expect("inbound verdict");
    assert_eq!(inbound_status, "FAILED");
    let error = error.expect("the failure reason is recorded on the row");
    // The trace says WHY, in the BOUNDED vocabulary #623 installed and #780 extended to this door.
    // It used to be `context.detail: <free text>`; `inbound_messages.error` is a 90-day durable
    // column, so the reason is now a closed pair and the diagnostic prose goes to the log. This is
    // a STRONGER assertion than the one it replaces: a closed set can be asserted exactly, and the
    // absence of free text is asserted too, which makes this a second leak canary on the one path
    // where a provider-shaped string could reach the column.
    assert_eq!(
        error["context"]["seam"].as_str(),
        Some("COMMAND_PAYLOAD"),
        "the seam names WHERE it broke: {error}"
    );
    assert_eq!(
        error["context"]["reason"].as_str(),
        Some("PAYLOAD_UNDECODABLE"),
        "the reason names WHY, in a vocabulary support can act on: {error}"
    );
    assert!(
        error["context"]["detail"].is_null(),
        "no free text may reach the 90-day error column (#623): {error}"
    );

    // (b) Reconciled onto the mirror as FAILED: nothing claims a sync, and the payload survives.
    let reconcile = worker.run_once().await.expect("reconcile pass");
    assert_eq!(reconcile.resolved, 1);
    let (mirror_status, payload, synced_at): (
        String,
        Option<serde_json::Value>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT status, payload, synced_at FROM external_sirene_restaurants \
          WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("reconciled row");
    assert_eq!(mirror_status, "FAILED");
    assert!(payload.is_some(), "a failed delivery is precisely when the raw record is still needed");
    assert!(synced_at.is_none(), "and nothing may claim it reached the domain");

    // (c) The row is checkpointed: the next pass has nothing to do — the churn is gone.
    let replay = worker.run_once().await.expect("no-op drain");
    assert_eq!(replay.processed, 0);
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("count events");
    assert_eq!(events, 0, "nothing reached the domain");
}

/// The worker does NOT know, at hand-over, whether the aggregate accepted the record — since
/// ADR-20260728-011344 the register path enqueues an inbound FACT and the MailboxWorker delivers
/// it later. So `STAGED -> SYNCED` has to be resolved from the aggregate's verdict on a subsequent pass,
/// and this pins that it actually happens: without it the mirror would sit on STAGED forever, or (worse)
/// claim a success nobody observed.
///
/// The join needs no extra bookkeeping — the ACL already writes
/// `inbound_messages.external_id = '{siret}:{payload_hash}'`, and both halves are columns on the row.
#[tokio::test]
async fn staged_rows_resolve_to_synced_from_the_aggregates_verdict() {
    let Some(db) = crate::common::TestDb::acquire("sync_sirene_worker").await else { return };
    let pool = db.pool();
    let worker = SireneSyncWorker::new(pool.clone());

    stage_row(&pool, "A").await;
    worker.run_once().await.expect("first drain stages the fact");

    let (status, synced_at): (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT status, synced_at FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("staged row");
    assert_eq!(status, "STAGED");
    assert!(synced_at.is_none(), "nothing has reached the domain yet, so nothing may claim a sync time");
    let payload: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("staged row payload");
    assert!(payload.is_some(), "the payload survives hand-over — the verdict is not in yet");

    // The mailbox worker delivers it and the aggregate decides. Simulated here by writing the
    // verdict the real delivery writes — SUCCEEDED.
    let updated = sqlx::query(
        "UPDATE inbound_messages SET status = 'SUCCEEDED', completed_at = now() WHERE source = 'sirene'",
    )
    .execute(&pool)
    .await
    .expect("simulate delivery")
    .rows_affected();
    assert_eq!(updated, 1, "the register path staged exactly one inbound fact");

    let summary = worker.run_once().await.expect("second drain reconciles");
    assert_eq!(summary.resolved, 1, "the verdict is read back onto the mirror");

    let (status, synced_at): (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT status, synced_at FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("resolved row");
    assert_eq!(status, "SYNCED", "the aggregate accepted it, so now the mirror may say so");
    assert!(synced_at.is_some(), "and records WHEN it reached the domain");
    // Only NOW is the payload spent — dropped in the same statement that recorded the certainty, never
    // on the strength of an assumption made earlier.
    let payload: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("resolved row payload");
    assert!(payload.is_none(), "confirmed synced — the payload is finally released");
}

/// A no-change verdict is a SUCCESS, not a failure. The aggregate folding "nothing moved" (IGNORED) means
/// the record reached the domain and the domain is now correct about it — conflating that with a failure
/// is what once made a sweep unable to tell 200,000 registrations from 200,000 no-ops.
#[tokio::test]
async fn an_ignored_verdict_still_counts_as_synced() {
    let Some(db) = crate::common::TestDb::acquire("sync_sirene_worker").await else { return };
    let pool = db.pool();
    let worker = SireneSyncWorker::new(pool.clone());

    stage_row(&pool, "A").await;
    worker.run_once().await.expect("stage the fact");
    // IGNORED: the aggregate decided nothing had changed.
    sqlx::query("UPDATE inbound_messages SET status = 'IGNORED', completed_at = now() WHERE source = 'sirene'")
        .execute(&pool)
        .await
        .expect("simulate a no-change verdict");

    worker.run_once().await.expect("reconcile");
    let status: String = sqlx::query_scalar(
        "SELECT status FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("row");
    assert_eq!(status, "SYNCED", "'nothing changed' is a real answer, not a failure");
}

/// The quarantine has to actually STOP the retry, or it is just a label.
///
/// A failed sync deliberately leaves the row pending WITH its payload — the retry needs something to
/// translate — so nothing in the pending predicate ever excludes a permanently-broken row. It would be
/// re-attempted on every pass forever, burning the sweep's budget and emitting an error nobody acts on.
/// (Not hypothetical: the 605-row SlugAlreadyTaken log storm was exactly this shape.) So the drain
/// filters on `status <> 'POISON'`, and this pins it.
#[tokio::test]
async fn a_poisoned_row_is_skipped_by_the_drain_and_keeps_its_payload() {
    let Some(db) = crate::common::TestDb::acquire("sync_sirene_worker").await else { return };
    let pool = db.pool();
    let worker = SireneSyncWorker::new(pool.clone());

    // A pending row that has already exhausted its attempts.
    stage_row(&pool, "A").await;
    sqlx::query(
        "UPDATE external_sirene_restaurants SET status = 'POISON', attempt_sync_retry_count = 10",
    )
    .execute(&pool)
    .await
    .expect("quarantine the row");

    let summary = worker.run_once().await.expect("drain");
    assert_eq!(summary.processed, 0, "a quarantined row is not re-attempted");

    let (payload, status): (Option<serde_json::Value>, String) = sqlx::query_as(
        "SELECT payload, status FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("row");
    assert!(payload.is_some(), "its payload is the evidence needed to diagnose it — never dropped");
    assert_eq!(status, "POISON", "and it stays quarantined until INSEE sends something different");
}

/// A FAILED verdict must not cost the payload. The record still has to be re-translated, and the raw
/// staging copy is the only original we hold — the inbound row carries the TRANSLATED form, which is
/// exactly what is suspect when a delivery fails.
#[tokio::test]
async fn a_failed_verdict_keeps_the_payload() {
    let Some(db) = crate::common::TestDb::acquire("sync_sirene_worker").await else { return };
    let pool = db.pool();
    let worker = SireneSyncWorker::new(pool.clone());

    stage_row(&pool, "A").await;
    worker.run_once().await.expect("stage the fact");
    // FAILED: the delivery could not apply the fact.
    sqlx::query("UPDATE inbound_messages SET status = 'FAILED', completed_at = now() WHERE source = 'sirene'")
        .execute(&pool)
        .await
        .expect("simulate a failed delivery");

    worker.run_once().await.expect("reconcile");
    let (status, payload, synced_at): (String, Option<serde_json::Value>, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as(
            "SELECT status, payload, synced_at FROM external_sirene_restaurants \
              WHERE siret = '85242109900021'",
        )
        .fetch_one(&pool)
        .await
        .expect("row");
    assert_eq!(status, "FAILED");
    assert!(payload.is_some(), "a failed delivery is precisely when the raw record is still needed");
    assert!(synced_at.is_none(), "and nothing may claim it reached the domain");
}

/// The drain must handle a MIXED batch in one pass and get every group right.
///
/// The per-row loop this replaced did one mailbox INSERT plus one staging UPDATE per row, strictly
/// sequential — measured at ~628 rows/min in production against ~3,800/min of ingest, with ~99% of the
/// wall-clock being round-trip latency rather than work. The batched form classifies the whole batch as
/// pure CPU and then writes it in two statements.
///
/// Every other test in this file drains one or two rows, so none of them would notice a multi-row
/// statement being wrong — a bad `UNNEST` join or a mis-zipped enqueue result would pass all of them.
/// This one uses 25 rows across all three classifications at once, which is what actually exercises the
/// batch: the mailbox insert, the `UNNEST` staging update, and the correspondence between them.
#[tokio::test]
async fn a_mixed_batch_drains_in_one_pass_with_every_group_marked_correctly() {
    let Some(db) = crate::common::TestDb::acquire("sync_sirene_worker").await else { return };
    let pool = db.pool();
    let worker = SireneSyncWorker::new(pool.clone());

    // 20 mappable active records, each a DIFFERENT SIRET — and therefore a different aggregate, which
    // is what makes batching safe in the first place.
    for i in 0..20u32 {
        let siret = format!("8524210990{:04}", i);
        let mut payload = sample_payload("A");
        payload["siret"] = serde_json::json!(siret);
        stage_row_with_payload(&pool, &siret, payload).await;
    }
    // 3 that parse but the ACL cannot map: no enseigne and no denomination anywhere ⇒ no usable name.
    for i in 0..3u32 {
        let siret = format!("8524210991{:04}", i);
        stage_row_with_payload(
            &pool,
            &siret,
            serde_json::json!({
                "siret": siret,
                "adresseEtablissement": { "codePostalEtablissement": "37000",
                                          "libelleCommuneEtablissement": "TOURS" },
                "periodesEtablissement": [ { "dateFin": null,
                                             "etatAdministratifEtablissement": "A",
                                             "activitePrincipaleEtablissement": "56.10A" } ]
            }),
        )
        .await;
    }
    // 2 that do not deserialize at all — the payload is the only evidence of what arrived.
    for i in 0..2u32 {
        let siret = format!("8524210992{:04}", i);
        stage_row_with_payload(&pool, &siret, serde_json::json!({ "unexpected": "shape" })).await;
    }

    let summary = worker.run_once().await.expect("batched drain");
    assert_eq!(summary.processed, 25, "every staged row is accounted for exactly once");
    assert_eq!(summary.registered, 20, "the mappable records were handed to the mailbox");
    assert_eq!(summary.skipped, 5, "3 ACL-unmappable + 2 unparsable");
    assert_eq!(summary.failed, 0);

    let counts: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT status, count(*), count(payload) FROM external_sirene_restaurants GROUP BY status ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .expect("status breakdown");
    assert_eq!(
        counts,
        vec![
            // Handed over, verdict not in yet — payload SURVIVES until the aggregate confirms (#240).
            ("STAGED".to_string(), 20, 20),
            // Evidence of why the record was unusable — never discarded (#231 D3).
            ("UNMAPPABLE".to_string(), 5, 5),
        ],
        "each classification lands in its own group, with the right payload disposition"
    );

    // 20 mailbox rows, one per aggregate — the multi-row INSERT wrote them all, not just the first.
    let staged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM inbound_messages WHERE source = 'sirene' AND message_type = 'RestaurantRegistered'",
    )
    .fetch_one(&pool)
    .await
    .expect("mailbox count");
    assert_eq!(staged, 20);
    let distinct_lanes: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT actor_id) FROM inbound_messages WHERE source = 'sirene'",
    )
    .fetch_one(&pool)
    .await
    .expect("lane count");
    assert_eq!(distinct_lanes, 20, "one lane per SIRET — no two rows collapsed onto the same aggregate");

    // Re-draining is a no-op: every row is checkpointed, so the batch cannot double-stage.
    let replay = worker.run_once().await.expect("replay");
    assert_eq!(replay.processed, 0, "a batched drain checkpoints every row it touched");
}
