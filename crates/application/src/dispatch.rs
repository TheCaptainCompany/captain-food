//! Worker-side journaling dispatch (ADR-20260720-015300) — the reusable plumbing that makes the
//! journal invariant true for EVERY channel: *all command submissions converge on `command_journal`,
//! whatever their origin*. The GraphQL resolvers journal inline (`channel: GRAPHQL`, generated
//! dispatch); on-app workers — the HubRise enricher, the SIRENE sync — route their sends through
//! [`dispatch_journaled`] with `channel: WORKER` instead of calling handlers directly.
//!
//! The worker contract differs from GraphQL's acceptance-first contract in two ways:
//!
//! - **Synchronous**: there is no caller waiting on a `MutationAcceptance`, so the handler runs in
//!   line (no spawn) and the journal row is completed before returning — the worker's own
//!   retry/skip bookkeeping consumes the outcome directly.
//! - **Deterministic `message_id`s**: workers derive the id from the *inbound fact's identity*
//!   (e.g. UUIDv5 of the HubRise callback id + command type), so a webhook redelivery or a
//!   re-drained staging row replays the SAME id and dedupes on the journal instead of
//!   double-applying ([`JournaledOutcome::Deduplicated`]). One deliberate deviation: a duplicate
//!   whose original completed **FAILED** (technical failure, or swept stale) is RE-EXECUTED under
//!   the same id — for a worker, redelivery IS the retry, and a client-style "submit a fresh id"
//!   escape hatch does not exist.
//!
//! Journals never write `domain_events` (the handler appends via the write-side `Repository`);
//! events appended by a journaled send carry `cause_id = message_id`, closing the
//! staging-mirror → `command_journal` → `domain_events` causality chain.

use domain::generated::scalars::CommandJournalStatus;
use domain::shared::errors::DomainError;

use crate::journal::{CommandJournal, CommandJournalEntry, JournalInsertOutcome};

/// What [`dispatch_journaled`] did with one worker command send.
#[derive(Debug)]
pub enum JournaledOutcome {
    /// The handler ran (fresh insert, or a retry of a FAILED duplicate); the journal row was
    /// completed (`SUCCEEDED` / `REJECTED` / `FAILED`) from this result.
    Executed(Result<(), DomainError>),
    /// Same `message_id` + same payload already journaled as SUCCEEDED / REJECTED / RECEIVED —
    /// the send was skipped (idempotent redelivery; RECEIVED = still in flight elsewhere, the
    /// stale sweep will flip it FAILED if that flight crashed).
    Deduplicated(CommandJournalStatus),
    /// Same `message_id` but a DIFFERENT payload: never dispatched. For a worker this means the
    /// source data changed under a redelivered identity (or a keying bug) — the caller logs and
    /// skips; the changed data arrives under its own fresh identity.
    PayloadConflict { existing_status: CommandJournalStatus },
}

/// Journal `entry`, then run `handler` and complete the row — the WORKER-channel counterpart of the
/// generated GraphQL dispatch. Only journal-store failures propagate as `Err`; the handler's own
/// outcome (including rejections and technical failures) is journaled and returned inside
/// [`JournaledOutcome::Executed`].
pub async fn dispatch_journaled<F, Fut>(
    journal: &dyn CommandJournal,
    entry: CommandJournalEntry,
    handler: F,
) -> Result<JournaledOutcome, DomainError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), DomainError>>,
{
    match journal.insert(&entry).await? {
        JournalInsertOutcome::Duplicate { status, payload_hash } => {
            if payload_hash != entry.payload_hash {
                return Ok(JournaledOutcome::PayloadConflict { existing_status: status });
            }
            if status != CommandJournalStatus::FAILED {
                return Ok(JournaledOutcome::Deduplicated(status));
            }
            // FAILED replay: re-execute under the same id and re-complete the original row.
        }
        JournalInsertOutcome::Inserted => {}
    }
    let outcome = handler().await;
    let (status, error) = completion_of(&outcome);
    journal.complete(entry.message_id, status, error).await?;
    Ok(JournaledOutcome::Executed(outcome))
}

/// Map a handler outcome onto the journal's terminal transition — the same discrimination the
/// generated GraphQL dispatch applies: a catalogued errors.yaml rejection is REJECTED (`{code,
/// context}` surfaced as `Operation.errorCode`); everything else is FAILED as the generic
/// `Internal` (adapter detail never leaks into the journal's error surface).
fn completion_of(
    outcome: &Result<(), DomainError>,
) -> (CommandJournalStatus, Option<serde_json::Value>) {
    match outcome {
        Ok(()) => (CommandJournalStatus::SUCCEEDED, None),
        Err(DomainError::Rejected { code, context }) => (
            CommandJournalStatus::REJECTED,
            Some(serde_json::json!({ "code": code, "context": context })),
        ),
        // Legacy "<Code>: <detail>" string invariants: a catalogued prefix is a rejection.
        Err(DomainError::Invariant(msg)) => {
            let code = msg.split(':').next().map(str::trim).unwrap_or("");
            if domain::generated::errors::find(code).is_some() {
                (
                    CommandJournalStatus::REJECTED,
                    Some(serde_json::json!({ "code": code, "context": { "detail": msg } })),
                )
            } else {
                internal_completion()
            }
        }
        Err(DomainError::Repository(_)) => internal_completion(),
    }
}

fn internal_completion() -> (CommandJournalStatus, Option<serde_json::Value>) {
    let def = domain::generated::errors::INTERNAL;
    (CommandJournalStatus::FAILED, Some(serde_json::json!({ "code": def.code, "context": {} })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::mem::MemCommandJournal;
    use crate::journal::payload_hash;
    use domain::generated::scalars::CommandChannel;
    use serde_json::json;

    fn worker_entry(message_id: uuid::Uuid, payload: serde_json::Value) -> CommandJournalEntry {
        CommandJournalEntry {
            message_id,
            correlation_id: message_id,
            cause_id: Some(uuid::Uuid::new_v4()),
            session_id: None,
            trace_id: None,
            user_id: Some(uuid::Uuid::new_v4()),
            user_type: 6,
            channel: CommandChannel::WORKER,
            command_type: "ImportCatalog".into(),
            payload_hash: payload_hash(&payload),
            payload,
        }
    }

    #[tokio::test]
    async fn fresh_send_executes_and_completes_succeeded() {
        let journal = MemCommandJournal::default();
        let id = uuid::Uuid::new_v4();
        let out = dispatch_journaled(&journal, worker_entry(id, json!({ "x": 1 })), || async {
            Ok(())
        })
        .await
        .unwrap();
        assert!(matches!(out, JournaledOutcome::Executed(Ok(()))));
        let row = journal.by_message(id).await.unwrap().unwrap();
        assert_eq!(row.status, CommandJournalStatus::SUCCEEDED);
        assert_eq!(row.entry.channel, CommandChannel::WORKER);
    }

    #[tokio::test]
    async fn redelivery_of_a_succeeded_send_dedupes_without_running_the_handler() {
        let journal = MemCommandJournal::default();
        let id = uuid::Uuid::new_v4();
        dispatch_journaled(&journal, worker_entry(id, json!({ "x": 1 })), || async { Ok(()) })
            .await
            .unwrap();
        let out = dispatch_journaled(&journal, worker_entry(id, json!({ "x": 1 })), || async {
            panic!("handler must not run on a deduplicated redelivery")
        })
        .await
        .unwrap();
        assert!(matches!(out, JournaledOutcome::Deduplicated(CommandJournalStatus::SUCCEEDED)));
    }

    #[tokio::test]
    async fn rejection_completes_rejected_with_the_errors_yaml_code() {
        let journal = MemCommandJournal::default();
        let id = uuid::Uuid::new_v4();
        let out = dispatch_journaled(&journal, worker_entry(id, json!({ "x": 1 })), || async {
            Err(DomainError::Rejected {
                code: "CatalogNotFound".into(),
                context: json!({ "catalogId": "c1" }),
            })
        })
        .await
        .unwrap();
        assert!(matches!(out, JournaledOutcome::Executed(Err(_))));
        let row = journal.by_message(id).await.unwrap().unwrap();
        assert_eq!(row.status, CommandJournalStatus::REJECTED);
        assert_eq!(row.error.unwrap()["code"], "CatalogNotFound");
        // A redelivery of a definitive rejection is deduplicated, not re-run.
        let out = dispatch_journaled(&journal, worker_entry(id, json!({ "x": 1 })), || async {
            panic!("rejection is definitive")
        })
        .await
        .unwrap();
        assert!(matches!(out, JournaledOutcome::Deduplicated(CommandJournalStatus::REJECTED)));
    }

    #[tokio::test]
    async fn failed_duplicate_is_retried_under_the_same_message_id() {
        let journal = MemCommandJournal::default();
        let id = uuid::Uuid::new_v4();
        let out = dispatch_journaled(&journal, worker_entry(id, json!({ "x": 1 })), || async {
            Err(DomainError::Repository("db unreachable".into()))
        })
        .await
        .unwrap();
        assert!(matches!(out, JournaledOutcome::Executed(Err(_))));
        assert_eq!(
            journal.by_message(id).await.unwrap().unwrap().status,
            CommandJournalStatus::FAILED
        );
        // Redelivery IS the worker's retry: the handler runs again and the SAME row completes.
        let out = dispatch_journaled(&journal, worker_entry(id, json!({ "x": 1 })), || async {
            Ok(())
        })
        .await
        .unwrap();
        assert!(matches!(out, JournaledOutcome::Executed(Ok(()))));
        assert_eq!(
            journal.by_message(id).await.unwrap().unwrap().status,
            CommandJournalStatus::SUCCEEDED
        );
    }

    #[tokio::test]
    async fn changed_payload_under_a_replayed_id_is_a_conflict_never_dispatched() {
        let journal = MemCommandJournal::default();
        let id = uuid::Uuid::new_v4();
        dispatch_journaled(&journal, worker_entry(id, json!({ "x": 1 })), || async { Ok(()) })
            .await
            .unwrap();
        let out = dispatch_journaled(&journal, worker_entry(id, json!({ "x": 2 })), || async {
            panic!("a conflicting payload must never reach the handler")
        })
        .await
        .unwrap();
        assert!(matches!(
            out,
            JournaledOutcome::PayloadConflict {
                existing_status: CommandJournalStatus::SUCCEEDED
            }
        ));
    }
}
