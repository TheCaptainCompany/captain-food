//! Resolver-level behavior freeze for the TYPED actor-client send (#284 slice 2,
//! PROP-20260728-152752 §2.1): a mutation executed against the real generated schema must land on
//! the mailbox as ONE row whose every column matches what the shared `command_entry` constructor
//! builds — channel GRAPHQL, the actor's lane and frozen partition, the caller's envelope — and
//! the acceptance / dedupe-replay / payload-Conflict contract (ADR-20260720-015500) must be
//! byte-identical to what the retired inline `MailboxEntry` construction produced. Modeled on
//! `crates/infrastructure/tests/actor_clients.rs`, but through the SCHEMA: this is the proof the
//! resolvers still speak the same acceptance contract after the typed-client rewrite. In-memory
//! mailbox double; no Postgres, never skips.

use std::sync::Arc;

use application::mailbox::mem::MemMailbox;
use domain::generated::commands::{AddCartLine, CartLine};
use domain::generated::scalars::{CartId, CartLineId, OfferId, RestaurantId, SessionId};
use server::graphql_acl::RequestRole;

/// An `EventStore` the acceptance path must NEVER reach: the mailbox-routed resolvers enqueue and
/// answer PENDING — the worker (absent here) owns delivery. Any call is the test failing loudly.
struct UntouchableEventStore;

#[async_trait::async_trait]
impl application::ports::EventStore for UntouchableEventStore {
    async fn append(
        &self,
        stream_name: &str,
        _expected_version: i64,
        _events: &[domain::generated::events::DomainEvent],
        _actor: &application::ports::Actor,
    ) -> Result<i64, domain::shared::errors::DomainError> {
        panic!("the acceptance path must not append events (stream {stream_name})");
    }

    async fn load(
        &self,
        stream_name: &str,
    ) -> Result<(Vec<domain::generated::events::DomainEvent>, i64), domain::shared::errors::DomainError>
    {
        panic!("the acceptance path must not load streams (stream {stream_name})");
    }
}

/// A `SlugReservationRepository` that grants every request — this test never configures a slug;
/// the field only has to be inhabited (same rationale as `graphql_write_path.rs`).
struct AlwaysFreeSlugs;

#[async_trait::async_trait]
impl application::queries::SlugReservationRepository for AlwaysFreeSlugs {
    async fn reserve(
        &self,
        _slug: domain::generated::scalars::Slug,
        _restaurant_id: domain::generated::scalars::RestaurantId,
    ) -> Result<bool, domain::shared::errors::DomainError> {
        Ok(true)
    }
    async fn release(
        &self,
        _slug: domain::generated::scalars::Slug,
        _restaurant_id: domain::generated::scalars::RestaurantId,
    ) -> Result<(), domain::shared::errors::DomainError> {
        Ok(())
    }
}

/// The real generated schema over write-side in-memory doubles only (no read repos, no DB): the
/// mutation resolvers pull exactly `Arc<dyn Mailbox>` from ctx, so a `MemMailbox` observes every
/// column the typed client writes.
fn schema_over(
    mailbox: Arc<dyn application::mailbox::Mailbox>,
) -> server::graphql_schema::CaptainSchema {
    server::graphql_schema::build_schema(
        None,
        Some(server::graphql_schema::WriteDeps {
            event_store: Arc::new(UntouchableEventStore),
            ownership: Arc::new(infrastructure::FailClosedGoogleOwnershipVerifier),
            gbp_probe: Arc::new(infrastructure::UnverifiedGbpOrderLinkProbe),
            auth_provider: Arc::new(infrastructure::FailClosedIdentityService),
            payments: Arc::new(infrastructure::FailClosedPaymentGateway),
            pm_state: Arc::new(application::generated::pm_state::mem::MemPaymentProcessState::default()),
            refund_state: Arc::new(application::generated::pm_state::mem::MemRefundProcessState::default()),
            journal: Arc::new(application::journal::mem::MemCommandJournal::default()),
            mailbox,
            status_bus: infrastructure::OperationStatusBus::default(),
            auth_sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
            slug_reservations: Arc::new(AlwaysFreeSlugs),
            pm_mailbox_delivery: false,
        }),
        None,
    )
}

#[tokio::test]
async fn typed_send_lands_the_command_entry_row_and_keeps_the_acceptance_contract() {
    let mem = Arc::new(MemMailbox::default());
    let schema = schema_over(mem.clone());

    let cart_id = uuid::Uuid::from_u128(0xCA57);
    let restaurant_id = uuid::Uuid::from_u128(0xF00D);
    let line_id = uuid::Uuid::from_u128(0x11);
    let offer_id = uuid::Uuid::from_u128(0x0FFE);
    let session = uuid::Uuid::from_u128(0x5E55);
    let message_id = uuid::Uuid::from_u128(0x1);

    let mutation = |quantity: i64| {
        format!(
            r#"mutation {{ addCartLine(input: {{ cartId: "{cart_id}", restaurantId: "{restaurant_id}", sessionId: "{session}", line: {{ cartLineId: "{line_id}", offerId: "{offer_id}", quantity: {quantity} }} }}, metadata: {{ messageId: "{message_id}" }}) {{ messageId correlationId sessionId operationStatus duplicate }} }}"#
        )
    };

    // 1) Fresh send: the uniform PENDING acceptance, and the echoed envelope.
    let resp = schema
        .execute(
            async_graphql::Request::new(mutation(2))
                .data(RequestRole::Public)
                .data(server::graphql_session::SessionHeader(Some(session))),
        )
        .await;
    assert!(resp.errors.is_empty(), "mutation errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json data");
    let acceptance = &data["addCartLine"];
    assert_eq!(acceptance["operationStatus"], "PENDING");
    assert_eq!(acceptance["duplicate"], false);
    assert_eq!(acceptance["messageId"], message_id.to_string());
    assert_eq!(acceptance["correlationId"], message_id.to_string(), "defaults to messageId");
    assert_eq!(acceptance["sessionId"], session.to_string());

    // 2) THE ROW: every column the typed client wrote, asserted against the same contract the
    //    worker-channel drift guard pins (actor_clients.rs) — with channel GRAPHQL and the
    //    request envelope. The payload is the DOMAIN COMMAND's own serde form (#284 slice 2):
    //    the typed value the resolver validated is the value that was sent, so the omitted
    //    `selectedOptionIds` input key lands as the command's defaulted `[]`.
    let row = mem.entry(message_id).expect("one mailbox row keyed by the supplied messageId");
    assert_eq!(row.kind, "COMMAND");
    assert_eq!(row.actor_type, "Cart");
    assert_eq!(row.actor_id, cart_id, "the lane is the declared identity property (cartId)");
    assert_eq!(
        row.partition,
        actor_runtime::stable_partition(&cart_id, 100),
        "the FROZEN partition over the Cart mailbox width"
    );
    assert_eq!(row.message_type, "AddCartLine");
    assert_eq!(row.channel, "GRAPHQL");
    assert_eq!(row.user_id, None, "anonymous PUBLIC caller");
    assert_eq!(row.user_type, "PUBLIC");
    assert_eq!(row.correlation_id, message_id);
    assert_eq!(row.cause_id, None);
    assert_eq!(row.session_id, Some(session), "the X-SESSION-ID ownership scope rides the row");
    assert_eq!(row.trace_id, None);
    assert_eq!(row.source, None);
    assert_eq!(row.external_id, None);
    let expected_cmd = AddCartLine {
        cart_id: CartId(cart_id),
        restaurant_id: RestaurantId(restaurant_id),
        line: CartLine {
            cart_line_id: CartLineId(line_id),
            offer_id: OfferId(offer_id),
            quantity: 2,
            selected_option_ids: vec![],
        },
        session_id: SessionId(session),
    };
    let expected_payload = serde_json::to_value(&expected_cmd).expect("serialize command");
    assert_eq!(row.payload, expected_payload, "payload = the typed command's serde form");
    assert_eq!(
        row.payload_hash,
        application::journal::payload_hash(&expected_payload),
        "hash over the payload as stored"
    );

    // 3) Idempotent replay: SAME messageId + SAME input → duplicate: true with the original's
    //    status (RECEIVED reads as PENDING), and nothing new on the mailbox.
    let resp = schema
        .execute(
            async_graphql::Request::new(mutation(2))
                .data(RequestRole::Public)
                .data(server::graphql_session::SessionHeader(Some(session))),
        )
        .await;
    assert!(resp.errors.is_empty(), "replay errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json data");
    assert_eq!(data["addCartLine"]["duplicate"], true);
    assert_eq!(data["addCartLine"]["operationStatus"], "PENDING");
    assert_eq!(mem.entries().len(), 1, "the replay enqueued nothing");

    // 4) SAME messageId + DIFFERENT payload: the synchronous Conflict (a client bug, never a
    //    silent overwrite), and still nothing new on the mailbox.
    let resp = schema
        .execute(
            async_graphql::Request::new(mutation(3))
                .data(RequestRole::Public)
                .data(server::graphql_session::SessionHeader(Some(session))),
        )
        .await;
    assert_eq!(resp.errors.len(), 1, "expected the Conflict: {:?}", resp.errors);
    let ext = resp.errors[0].extensions.as_ref().expect("extensions");
    assert_eq!(ext.get("code"), Some(&async_graphql::Value::from("Conflict")));
    assert_eq!(mem.entries().len(), 1, "the conflict enqueued nothing");
}
