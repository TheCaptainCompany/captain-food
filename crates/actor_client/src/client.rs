//! The generic READ door over operation status (PROP-20260802-130500 D4, #290 phase 1).
//!
//! The shape follows the data (product-owner directive, 2026-08-02): **operation status is
//! generic to all operations** — `message_id` is globally unique and the outcome
//! (PENDING/SUCCEEDED/REJECTED/…) is an ENVELOPE-level fact carrying nothing actor-specific — so
//! it is deliberately NOT a method on the per-actor typed clients (that would pretend a generic
//! read is actor-specific, sixteen times), and there is no separate `OperationStatusClient`
//! concept. The split: **per-actor typed clients = the write side** (send/record/schedule/cancel,
//! where WHICH actor matters at compile time); **the one generic [`ActorClient`] = the read
//! side** (where it does not).
//!
//! This is the ONLY sanctioned read path over `inbound_messages` status: the `operationStatus`
//! query and the `operationStatusChanged` snapshot both resolve through it, and nobody SELECTs
//! the table (the D3 capability allowlist keeps SQL out of every crate that could try).
//!
//! `watch(message_id)` (the §2.1 response-bus subscription) is deliberately ABSENT for now: the
//! in-process `OperationStatusBus` lives in `infrastructure` — above this crate — keyed to the
//! legacy `CommandJournalStatus`; folding it down into this crate is a boundary move of its own,
//! recorded on #290 rather than improvised here.

use std::sync::Arc;

use domain::shared::errors::DomainError;

use crate::mailbox::{Mailbox, MailboxStatusRow};

/// The one generic, actor-agnostic client: holds the read capability over operation status.
pub struct ActorClient {
    mailbox: Arc<dyn Mailbox>,
}

impl ActorClient {
    pub fn new(mailbox: Arc<dyn Mailbox>) -> Self {
        Self { mailbox }
    }

    /// The status of one operation by its globally-unique acceptance handle — the row behind
    /// `operationStatus` / the `operationStatusChanged` snapshot. `None` = unknown identity; the
    /// OWNERSHIP decision (no existence oracle, ADR-20260720-015500) stays with the caller, which
    /// alone knows the request principal.
    pub async fn get_operation_status(
        &self,
        message_id: uuid::Uuid,
    ) -> Result<Option<MailboxStatusRow>, DomainError> {
        self.mailbox.by_message(message_id).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use domain::generated::scalars::InboundMessageStatus;

    use super::ActorClient;
    use crate::mailbox::mem::MemMailbox;
    use crate::mailbox::{Envelope, Mailbox};

    /// The read door answers with the very row the write door accepted — same identity, same
    /// status the mem worker left it in — and `None` for an unknown handle.
    #[tokio::test]
    async fn get_operation_status_reads_the_accepted_row_and_none_for_unknown() {
        let mem = Arc::new(MemMailbox::default());
        let restaurant_id = uuid::Uuid::from_u128(0xF00D);
        let message_id = uuid::Uuid::from_u128(0x0B5);
        let cmd = domain::generated::commands::MarkRestaurantClosed {
            restaurant_id: domain::generated::scalars::RestaurantId(restaurant_id),
            reason: None,
        };
        let writer = crate::generated::actor_clients::RestaurantClient::new(
            mem.clone() as Arc<dyn Mailbox>,
            restaurant_id,
        );
        writer
            .send(
                cmd,
                Envelope {
                    message_id,
                    correlation_id: message_id,
                    cause_id: None,
                    session_id: None,
                    trace_id: None,
                    user_id: None,
                    user_type: "EXTERNAL".into(),
                    channel: "WORKER".into(),
                },
            )
            .await
            .expect("typed send");

        let reader = ActorClient::new(mem.clone());
        let row = reader
            .get_operation_status(message_id)
            .await
            .expect("read")
            .expect("the accepted row is visible through the read door");
        assert_eq!(row.message_id, message_id);
        assert_eq!(row.correlation_id, message_id);
        assert_eq!(row.status, InboundMessageStatus::RECEIVED);

        assert!(
            reader
                .get_operation_status(uuid::Uuid::from_u128(0xDEAD))
                .await
                .expect("read")
                .is_none(),
            "an unknown handle is None — the ownership/oracle policy stays with the caller"
        );
    }
}
