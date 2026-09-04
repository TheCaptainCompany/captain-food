//! #639 part C step 2c-ii — `Operation.message` is read in the CALLER's language, on both legs.
//!
//! The rider sign-in door renders its refusals on the screen, in French, naming the support
//! contact from the error's own context (ADR-20260830-213135). Until this slice every leg built
//! the message with `message_en`, so the sentence a French rider read was English. The code stays
//! the contract (`errorCode`); the message is presentation, derived at READ time from the mailbox
//! row's typed `{ code, context }` in the locale the transport injected (`RequestLocale`). Pinned
//! through the REAL schema with a SCRIPTED mailbox whose row is already terminal:
//!   (a) the poll leg (`operationStatus`) answers French under `RequestLocale::Fr`, with the
//!       support contact interpolated from the row's context — never from anything a screen spells;
//!   (b) the same request with NO locale datum keeps the pre-locale contract (English), so no
//!       schema-level assertion anywhere moved;
//!   (c) the push leg (`operationStatusChanged`, snapshot-first) answers French too — the two legs
//!       cannot disagree about the sentence, because both build from the durable row.

use std::sync::Arc;

use async_graphql::futures_util::StreamExt;
use async_graphql::Request;
use async_trait::async_trait;
use domain::generated::scalars as ds;
use domain::shared::errors::DomainError;
use server::graphql_acl::RequestRole;
use server::graphql_locale::RequestLocale;
use server::graphql_schema::build_schema;

const SUPPORT: &str = "support@captain.food";

fn acting(role: RequestRole) -> server::ActingRole {
    server::Principal::role_binding(role, "test-subject".to_string(), Some(uuid::Uuid::from_u128(0x639)))
        .acting_role(role)
}

/// A mailbox holding exactly ONE row, already REJECTED with the rider door's typed refusal.
struct RejectedRow {
    message_id: uuid::Uuid,
    session: uuid::Uuid,
}

#[async_trait]
impl actor_client::mailbox::Mailbox for RejectedRow {
    async fn insert(
        &self,
        _entry: &actor_client::mailbox::MailboxEntry,
        _access: actor_client::mailbox::MailboxAccess,
    ) -> Result<actor_client::mailbox::MailboxInsertOutcome, DomainError> {
        Ok(actor_client::mailbox::MailboxInsertOutcome::Inserted)
    }
    async fn by_message(
        &self,
        message_id: uuid::Uuid,
        _access: actor_client::mailbox::MailboxAccess,
    ) -> Result<Option<actor_client::mailbox::MailboxStatusRow>, DomainError> {
        if message_id != self.message_id {
            return Ok(None);
        }
        Ok(Some(actor_client::mailbox::MailboxStatusRow {
            message_id,
            correlation_id: message_id,
            status: ds::InboundMessageStatus::REJECTED,
            error: Some(serde_json::json!({
                "code": "RiderNotRegistered",
                "context": { "supportContact": SUPPORT }
            })),
            payload_hash: "h".into(),
            user_id: None,
            session_id: Some(self.session),
            received_at: chrono::Utc::now(),
            completed_at: Some(chrono::Utc::now()),
        }))
    }
    async fn schedule(
        &self,
        _entry: &actor_client::mailbox::MailboxEntry,
        _scheduled_at: chrono::DateTime<chrono::Utc>,
        _policy: actor_client::mailbox::ReschedulePolicy,
        _access: actor_client::mailbox::MailboxAccess,
    ) -> Result<actor_client::mailbox::MailboxScheduleOutcome, DomainError> {
        Ok(actor_client::mailbox::MailboxScheduleOutcome::Scheduled)
    }
    async fn cancel_scheduled(
        &self,
        _message_id: uuid::Uuid,
        _access: actor_client::mailbox::MailboxAccess,
    ) -> Result<bool, DomainError> {
        Ok(false)
    }
}

fn request(query: String, session: uuid::Uuid, mailbox: &Arc<dyn actor_client::mailbox::Mailbox>) -> Request {
    Request::new(query)
        .data(acting(RequestRole::Public))
        .data(server::graphql_session::SessionHeader(Some(session)))
        .data(mailbox.clone())
        .data(actor_client::OperationStatusBus::default())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_poll_leg_answers_in_the_callers_language_and_english_without_a_locale() {
    let schema = build_schema(None, None, None);
    let message_id = uuid::Uuid::now_v7();
    let session = uuid::Uuid::now_v7();
    let mailbox: Arc<dyn actor_client::mailbox::Mailbox> = Arc::new(RejectedRow { message_id, session });
    let query = format!(
        r#"query {{ operationStatus(input: {{ messageId: "{message_id}" }}) {{ status errorCode message }} }}"#
    );

    // (a) French, with the support contact from the row's context.
    let resp = schema.execute(request(query.clone(), session, &mailbox).data(RequestLocale::Fr)).await;
    assert!(resp.errors.is_empty(), "{:?}", resp.errors);
    let op = resp.data.into_json().expect("json")["operationStatus"].clone();
    assert_eq!(op["status"], "REJECTED");
    assert_eq!(op["errorCode"], "RiderNotRegistered");
    let message = op["message"].as_str().expect("a message").to_string();
    assert!(message.contains("compte livreur"), "French catalogue sentence expected, got: {message}");
    assert!(message.contains(SUPPORT), "the support contact is interpolated from context: {message}");
    assert!(!message.contains("{supportContact}"), "no raw placeholder: {message}");

    // (b) No locale datum: the pre-locale contract, English.
    let resp = schema.execute(request(query, session, &mailbox)).await;
    assert!(resp.errors.is_empty(), "{:?}", resp.errors);
    let op = resp.data.into_json().expect("json")["operationStatus"].clone();
    let message = op["message"].as_str().expect("a message").to_string();
    assert!(message.contains("rider account"), "English expected without a locale, got: {message}");
    assert!(message.contains(SUPPORT), "{message}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_push_leg_answers_in_the_same_language_as_the_poll_leg() {
    let schema = build_schema(None, None, None);
    let message_id = uuid::Uuid::now_v7();
    let session = uuid::Uuid::now_v7();
    let mailbox: Arc<dyn actor_client::mailbox::Mailbox> = Arc::new(RejectedRow { message_id, session });
    let query = format!(
        r#"subscription {{ operationStatusChanged(input: {{ messageId: "{message_id}" }}) {{ status errorCode message }} }}"#
    );
    let mut stream = schema.execute_stream(request(query, session, &mailbox).data(RequestLocale::Fr));
    // Snapshot-first: the terminal row arrives and the stream completes.
    let first = tokio::time::timeout(std::time::Duration::from_secs(10), stream.next())
        .await
        .expect("snapshot in time")
        .expect("stream item");
    assert!(first.errors.is_empty(), "{:?}", first.errors);
    let op = first.data.into_json().expect("json")["operationStatusChanged"].clone();
    assert_eq!(op["status"], "REJECTED");
    let message = op["message"].as_str().expect("a message").to_string();
    assert!(message.contains("compte livreur") && message.contains(SUPPORT), "{message}");
    let end = tokio::time::timeout(std::time::Duration::from_secs(10), stream.next()).await.expect("ends");
    assert!(end.is_none(), "a terminal snapshot completes the stream");
}
