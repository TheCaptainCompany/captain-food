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

use actor_client::mailbox::mem::MemMailbox;
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
    mailbox: Arc<dyn actor_client::mailbox::Mailbox>,
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
            mailbox,
            status_bus: actor_client::OperationStatusBus::default(),
            auth_sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
            slug_reservations: Arc::new(AlwaysFreeSlugs),
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
    // FULL destructure, no `..`, via the D5 EntryFixture mirror (PROP-20260802-130500):
    // `MailboxEntry` fields are private outside the actor_client crate now, and the fixture
    // conversions are themselves full-field (no `..`) inside that crate — so an 18th column
    // still breaks compilation before it can silently under-assert (the #289 review's
    // exhaustiveness finding, preserved across the boundary move).
    let actor_client::mailbox::fixtures::EntryFixture {
        message_id: row_message_id,
        kind,
        actor_type,
        actor_id,
        partition,
        message_type,
        payload: row_payload,
        payload_hash: row_payload_hash,
        channel,
        user_id,
        user_type,
        correlation_id,
        cause_id,
        session_id,
        trace_id,
        source,
        external_id,
    } = row.into_fixture();
    assert_eq!(row_message_id, message_id);
    assert_eq!(kind, "COMMAND");
    assert_eq!(actor_type, "Cart");
    assert_eq!(actor_id, cart_id, "the lane is the declared identity property (cartId)");
    assert_eq!(
        partition,
        actor_client::stable_partition(&cart_id, 5),
        "the FROZEN partition over the Cart mailbox width"
    );
    assert_eq!(message_type, "AddCartLine");
    assert_eq!(channel, "GRAPHQL");
    assert_eq!(user_id, None, "anonymous PUBLIC caller");
    assert_eq!(user_type, "PUBLIC");
    assert_eq!(correlation_id, message_id);
    assert_eq!(cause_id, None);
    assert_eq!(session_id, Some(session), "the X-SESSION-ID ownership scope rides the row");
    assert_eq!(trace_id, None);
    assert_eq!(source, None);
    assert_eq!(external_id, None);
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
    assert_eq!(row_payload, expected_payload, "payload = the typed command's serde form");
    assert_eq!(
        row_payload_hash,
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


/// The #289 review's BLOCKING finding, pinned in its post-#242-Runtime-D form: a retry of an
/// accepted PM command whose payload carries an ABSENT OPTIONAL must replay as `duplicate: true`,
/// never a synchronous Conflict. The bug was a hash-form mismatch — the null-stripped GraphQL input
/// hashes differently from the TYPED command form the mailbox row stores — and on the money path
/// (`placeOrder`/`approveRefund`) it means a 409 to a caller who did nothing wrong. The gated
/// second arm that made this a CROSS-arm problem is gone; the hash-form trap is not, because the
/// resolver still hashes an input form on the way in.
#[tokio::test]
async fn a_retry_with_an_absent_optional_replays_as_duplicate_not_conflict() {
    let mailbox = Arc::new(MemMailbox::default());
    let order_id = uuid::Uuid::from_u128(0x0D_0E);
    let message_id = uuid::Uuid::from_u128(0x7E57);
    // `reason` is deliberately ABSENT — the field whose explicit-null typed form diverges from the
    // null-stripped input form. With `reason` present the two forms coincide and this test would
    // prove nothing (which is why the DenyRefund coverage could not catch the bug).
    let mutation = format!(
        r#"mutation {{ approveRefund(input: {{ orderId: "{order_id}", amount: {{ amountCents: 500, currency: "EUR" }} }}, metadata: {{ messageId: "{message_id}" }}) {{ operationStatus duplicate }} }}"#
    );

    let schema = schema_over(mailbox.clone());
    let resp = schema
        .execute(async_graphql::Request::new(mutation.clone()).data(RequestRole::Admin))
        .await;
    assert!(resp.errors.is_empty(), "the first accept errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json data");
    assert_eq!(data["approveRefund"]["operationStatus"], "PENDING");
    assert_eq!(data["approveRefund"]["duplicate"], false);
    assert!(mailbox.entry(message_id).is_some(), "accepted onto the mailbox");

    // Same messageId + same input: a replay, not a client bug.
    let resp = schema
        .execute(async_graphql::Request::new(mutation).data(RequestRole::Admin))
        .await;
    assert!(resp.errors.is_empty(), "the retry must not Conflict: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json data");
    assert_eq!(
        data["approveRefund"]["duplicate"], true,
        "a committed acceptance replays as a duplicate"
    );
    assert_eq!(mailbox.entries().len(), 1, "the retry wrote nothing new anywhere");
}
