//! Persisted pending writes (#17) — the acceptance-first contract's durability half.
//!
//! ADR-20260720-015500's client rule: **persist the minted `messageId` until a terminal status is
//! observed**. The dispatcher (`actions.rs`) owns the two-step protocol; this module owns the
//! SURVIVAL of an in-flight intent across the exact V0 mobile failures — tab killed while Stripe
//! redirects, flaky radio forcing a reload, a double-tap racing a dead network:
//!
//!   * [`dispatch_persisted`] records the intent (messageId + action + full input) **BEFORE** the
//!     mutation is sent — a network failure between record and acceptance leaves the record, so the
//!     retry path still holds the id (recording after the send would lose exactly the crashes that
//!     matter).
//!   * [`retry`] re-dispatches a stored intent under its ORIGINAL messageId: the server journals
//!     nothing twice (`duplicate: true`) and polling converges on the first outcome — the
//!     duplicate-proof retry affordance of the issue.
//!   * [`resume_pending`] runs at boot: every stored id is re-resolved via `operationStatus` (an
//!     idempotent read, ownership-scoped by the restart-surviving session id, #12); terminal
//!     outcomes clear their record, a still-PENDING/unreachable one stays for the next boot.
//!
//! A record is cleared ONLY on a terminal outcome (SUCCEEDED / REJECTED / FAILED — rejection is an
//! outcome, not a loss). Technical errors (transport, poll exhaustion) keep it: the intent is still
//! open and the id must survive.
//!
//! Storage mirrors `session.rs`: localStorage on the browser path (one JSON key, origin-scoped so
//! each storefront keeps its own queue), an injectable in-memory double everywhere else. Storage
//! failures degrade to volatile — still correct, just less continuous.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::actions::{dispatch_with_id, ActionError, ActionOutcome, DispatchHandle, OperationStatus};
use crate::generated::data_layer::ActionKey;
use crate::graphql::Transport;

/// The single localStorage key the pending-write queue lives under (browser path).
pub const PENDING_STORAGE_KEY: &str = "captain.pending-writes";

/// One recorded in-flight write: everything needed to RETRY it (the full mutation input travels so
/// a resume can re-send verbatim — for `place_order` the input carries the client-minted `orderId`,
/// so the confirmation route is recoverable from the record alone).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingWrite {
    pub message_id: Uuid,
    pub action: ActionKey,
    pub input: Map<String, Value>,
}

/// The persistence seam — object-safe, injectable (the whole module tests against the memory
/// double; the browser impl is a thin JSON round-trip).
pub trait PendingStore {
    fn load(&self) -> Vec<PendingWrite>;
    fn save(&self, writes: &[PendingWrite]);
}

fn record(store: &dyn PendingStore, write: PendingWrite) {
    let mut writes = store.load();
    // Re-recording the same id is idempotent (a retry records nothing new).
    if !writes.iter().any(|w| w.message_id == write.message_id) {
        writes.push(write);
        store.save(&writes);
    }
}

fn clear(store: &dyn PendingStore, message_id: Uuid) {
    let mut writes = store.load();
    let before = writes.len();
    writes.retain(|w| w.message_id != message_id);
    if writes.len() != before {
        store.save(&writes);
    }
}

/// Dispatch an action with its intent PERSISTED first (see module docs for the ordering rationale).
/// Returns the acceptance handle; awaiting the verdict — and clearing the record — is
/// [`settle`]'s job, so a caller may navigate on acceptance and settle later.
pub async fn dispatch_persisted(
    transport: &dyn Transport,
    store: &dyn PendingStore,
    key: ActionKey,
    input: Map<String, Value>,
) -> Result<DispatchHandle, ActionError> {
    let message_id = Uuid::now_v7();
    record(store, PendingWrite { message_id, action: key, input: input.clone() });
    // A pre-acceptance failure (transport error, allowlist refusal) keeps the record for `retry` —
    // EXCEPT the non-dispatchable kinds, which can never succeed and would pin the queue forever.
    let result = dispatch_with_id(transport, key, input, message_id).await;
    if matches!(
        result,
        Err(ActionError::ClientSideAction(_))
            | Err(ActionError::AuthAction(_))
            | Err(ActionError::GapAction { .. })
            | Err(ActionError::UnboundMutation(_))
    ) {
        clear(store, message_id);
    }
    result
}

/// Resolve a handle's terminal outcome and clear its record. Technical failures (transport, poll
/// exhaustion) keep the record — the intent is still open.
pub async fn settle(
    transport: &dyn Transport,
    store: &dyn PendingStore,
    handle: &DispatchHandle,
) -> Result<ActionOutcome, ActionError> {
    settle_with(transport, store, handle, crate::actions::POLL_MAX_ATTEMPTS, crate::actions::POLL_INTERVAL)
        .await
}

/// [`settle`] with explicit poll bounds (tests use `Duration::ZERO`).
pub async fn settle_with(
    transport: &dyn Transport,
    store: &dyn PendingStore,
    handle: &DispatchHandle,
    max_attempts: u32,
    interval: std::time::Duration,
) -> Result<ActionOutcome, ActionError> {
    let outcome = handle.resolve_with(transport, max_attempts, interval).await?;
    clear(store, handle.message_id);
    Ok(outcome)
}

/// Re-dispatch a stored intent under its ORIGINAL messageId — the duplicate-proof retry.
pub async fn retry(
    transport: &dyn Transport,
    write: &PendingWrite,
) -> Result<DispatchHandle, ActionError> {
    dispatch_with_id(transport, write.action, write.input.clone(), write.message_id).await
}

/// Settle from a PUSHED `Operation` payload (#93: `operationStatusChanged` is the PRIMARY verdict
/// path; the bounded poll of [`settle`] is the fallback). Interprets the frame with the same
/// operation→outcome authority as the poll loop; a terminal verdict clears the record,
/// `Ok(None)` (PENDING / not-yet-readable) leaves it — nothing to await, the next frame or the
/// fallback poll carries the verdict.
pub fn settle_from_push(
    store: &dyn PendingStore,
    handle: &DispatchHandle,
    operation: &Value,
) -> Result<Option<ActionOutcome>, ActionError> {
    let outcome = crate::actions::outcome_from_operation(handle.message_id, operation)?;
    if outcome.is_some() {
        clear(store, handle.message_id);
    }
    Ok(outcome)
}

/// What [`resume_pending`] found for one stored write.
#[derive(Debug)]
pub enum ResumedWrite {
    /// A terminal verdict was read; the record is cleared.
    Settled { write: PendingWrite, outcome: ActionOutcome },
    /// Still PENDING / unreadable within the bounds; the record STAYS for the next boot or an
    /// explicit [`retry`].
    StillOpen { write: PendingWrite },
}

/// Boot-time recovery: re-resolve every stored messageId via `operationStatus` (idempotent read),
/// clearing the settled ones. Runs with explicit bounds — a resume must never spin a mobile radio
/// for 30 s per stale record; callers pass tight bounds (1–3 reads) and lean on [`retry`] for
/// records the journal has never seen.
pub async fn resume_pending(
    transport: &dyn Transport,
    store: &dyn PendingStore,
    max_attempts: u32,
    interval: std::time::Duration,
) -> Vec<ResumedWrite> {
    let mut resumed = Vec::new();
    for write in store.load() {
        let handle = DispatchHandle {
            message_id: write.message_id,
            duplicate: true, // by construction: the record exists, so the intent was (at least) attempted
            status_at_acceptance: OperationStatus::Pending,
        };
        match handle.resolve_with(transport, max_attempts, interval).await {
            Ok(outcome) => {
                clear(store, write.message_id);
                resumed.push(ResumedWrite::Settled { write, outcome });
            }
            // PENDING beyond the bound, or transport trouble: keep the record, report it open.
            Err(_) => resumed.push(ResumedWrite::StillOpen { write }),
        }
    }
    resumed
}

/// The in-memory store: the test double AND the native/SSR default (a server render never
/// persists client intents — the browser owns the queue, same split as `session.rs`).
#[derive(Default)]
pub struct MemoryPendingStore(std::sync::Mutex<Vec<PendingWrite>>);

impl PendingStore for MemoryPendingStore {
    fn load(&self) -> Vec<PendingWrite> {
        self.0.lock().expect("pending store mutex").clone()
    }
    fn save(&self, writes: &[PendingWrite]) {
        *self.0.lock().expect("pending store mutex") = writes.to_vec();
    }
}

/// The browser store (`hydrate` path): one JSON array under [`PENDING_STORAGE_KEY`]. Corrupt or
/// unavailable storage degrades to empty/volatile — self-healing, never a panic.
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub struct BrowserPendingStore;

#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
impl PendingStore for BrowserPendingStore {
    fn load(&self) -> Vec<PendingWrite> {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item(PENDING_STORAGE_KEY).ok().flatten())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }
    fn save(&self, writes: &[PendingWrite]) {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            if let Ok(json) = serde_json::to_string(writes) {
                let _ = storage.set_item(PENDING_STORAGE_KEY, &json);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql::test_support::FakeTransport;
    use crate::graphql::TransportError;
    use serde_json::json;
    use std::time::Duration;

    fn acceptance(status: &str, duplicate: bool) -> Value {
        json!({ "addCartLine": {
            "messageId": "00000000-0000-7000-8000-000000000000",
            "correlationId": "00000000-0000-7000-8000-000000000000",
            "causeId": null, "sessionId": null, "traceId": null,
            "operationStatus": status, "duplicate": duplicate,
        }})
    }

    fn operation(status: &str, error_code: Option<&str>) -> Value {
        json!({ "operationStatus": {
            "messageId": "00000000-0000-7000-8000-000000000000",
            "correlationId": "00000000-0000-7000-8000-000000000000",
            "status": status, "errorCode": error_code, "message": null,
            "occurredAt": "2026-07-24T12:00:00Z",
        }})
    }

    fn input() -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("offerId".into(), json!("offer-1"));
        m
    }

    #[tokio::test]
    async fn the_record_exists_before_the_send_and_survives_a_network_failure() {
        let store = MemoryPendingStore::default();
        let fake = FakeTransport::scripted(vec![Err(TransportError::Network("radio died".into()))]);
        let err = dispatch_persisted(&fake, &store, ActionKey::AddToCart, input()).await.unwrap_err();
        assert!(matches!(err, ActionError::Transport(_)));
        // THE contract: the id survived the crash window — the retry path still holds it.
        let writes = store.load();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].action, ActionKey::AddToCart);
        assert_eq!(writes[0].input["offerId"], json!("offer-1"));
    }

    #[tokio::test]
    async fn settle_clears_on_every_terminal_outcome_including_rejected() {
        for (status, code) in [("SUCCEEDED", None), ("REJECTED", Some("RestaurantPaused")), ("FAILED", None)] {
            let store = MemoryPendingStore::default();
            let fake = FakeTransport::scripted(vec![
                Ok(acceptance("PENDING", false)),
                Ok(operation(status, code)),
            ]);
            let handle = dispatch_persisted(&fake, &store, ActionKey::AddToCart, input()).await.unwrap();
            assert_eq!(store.load().len(), 1, "recorded while in flight");
            settle_with(&fake, &store, &handle, 5, Duration::ZERO).await.unwrap();
            assert!(store.load().is_empty(), "{status} is terminal — record must clear");
        }
    }

    #[tokio::test]
    async fn polling_exhaustion_keeps_the_record() {
        let store = MemoryPendingStore::default();
        let fake = FakeTransport::scripted(vec![
            Ok(acceptance("PENDING", false)),
            Ok(operation("PENDING", None)),
            Ok(operation("PENDING", None)),
        ]);
        let handle = dispatch_persisted(&fake, &store, ActionKey::AddToCart, input()).await.unwrap();
        let err = settle_with(&fake, &store, &handle, 2, Duration::ZERO).await.unwrap_err();
        assert!(matches!(err, ActionError::PollingExhausted { .. }));
        assert_eq!(store.load().len(), 1, "an open intent must keep its id");
    }

    #[tokio::test]
    async fn non_dispatchable_kinds_never_pin_the_queue() {
        let store = MemoryPendingStore::default();
        let fake = FakeTransport::scripted(vec![]);
        let err = dispatch_persisted(&fake, &store, ActionKey::Navigate, Map::new()).await.unwrap_err();
        assert!(matches!(err, ActionError::ClientSideAction(_)));
        assert!(store.load().is_empty(), "a kind that can never succeed must not be recorded");
    }

    #[tokio::test]
    async fn retry_reuses_the_original_message_id_and_converges_on_the_first_outcome() {
        let store = MemoryPendingStore::default();
        let fake = FakeTransport::scripted(vec![Err(TransportError::Network("radio died".into()))]);
        let _ = dispatch_persisted(&fake, &store, ActionKey::AddToCart, input()).await;
        let write = store.load().remove(0);

        // The retry: same messageId travels; the server echoes duplicate + the ORIGINAL's status.
        let fake = FakeTransport::scripted(vec![Ok(acceptance("SUCCEEDED", true))]);
        let handle = retry(&fake, &write).await.unwrap();
        assert_eq!(handle.message_id, write.message_id);
        assert!(handle.duplicate);
        assert_eq!(
            fake.call(0).1["metadata"]["messageId"],
            json!(write.message_id.to_string()),
            "the ORIGINAL id must travel — a fresh mint would double the order"
        );
        // Terminal echo resolves without polling; settle clears the record.
        let outcome = settle_with(&fake, &store, &handle, 5, Duration::ZERO).await.unwrap();
        assert!(matches!(outcome, ActionOutcome::Succeeded { .. }));
        assert!(store.load().is_empty());
    }

    #[tokio::test]
    async fn resume_settles_terminal_records_and_keeps_open_ones() {
        let store = MemoryPendingStore::default();
        // Two stored intents from a previous page lifetime.
        store.save(&[
            PendingWrite { message_id: Uuid::now_v7(), action: ActionKey::AddToCart, input: input() },
            PendingWrite { message_id: Uuid::now_v7(), action: ActionKey::PlaceOrder, input: Map::new() },
        ]);
        // Boot: the first resolves REJECTED (terminal), the second is still PENDING.
        let fake = FakeTransport::scripted(vec![
            Ok(operation("REJECTED", Some("OfferOutOfStock"))),
            Ok(operation("PENDING", None)),
        ]);
        let resumed = resume_pending(&fake, &store, 1, Duration::ZERO).await;
        assert_eq!(resumed.len(), 2);
        assert!(matches!(
            &resumed[0],
            ResumedWrite::Settled { outcome: ActionOutcome::Rejected { error_code, .. }, .. }
                if error_code == "OfferOutOfStock"
        ));
        assert!(matches!(&resumed[1], ResumedWrite::StillOpen { .. }));
        // Only the open intent remains stored.
        let remaining = store.load();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].action, ActionKey::PlaceOrder);
    }

    #[tokio::test]
    async fn a_pushed_terminal_operation_settles_and_clears_push_first() {
        // The #93 push path: an operationStatusChanged frame settles the write with NO poll.
        let store = MemoryPendingStore::default();
        let fake = FakeTransport::scripted(vec![Ok(acceptance("PENDING", false))]);
        let handle = dispatch_persisted(&fake, &store, ActionKey::AddToCart, input()).await.unwrap();
        assert_eq!(store.load().len(), 1);

        // A PENDING frame settles nothing and keeps the record.
        let pending_frame = json!({ "status": "PENDING", "errorCode": null, "message": null });
        assert!(settle_from_push(&store, &handle, &pending_frame).unwrap().is_none());
        assert_eq!(store.load().len(), 1);

        // The terminal frame settles + clears — one frame, zero operationStatus reads.
        let rejected = json!({ "status": "REJECTED", "errorCode": "OfferOutOfStock", "message": null });
        match settle_from_push(&store, &handle, &rejected).unwrap() {
            Some(ActionOutcome::Rejected { error_code, .. }) => {
                assert_eq!(error_code, "OfferOutOfStock")
            }
            other => panic!("expected the pushed rejection, got {other:?}"),
        }
        assert!(store.load().is_empty());
        assert_eq!(fake.call_count(), 1, "push settling must not touch the transport");
    }

    #[test]
    fn pending_writes_round_trip_through_json() {
        // The browser store is a serde round-trip of exactly this shape.
        let write = PendingWrite {
            message_id: Uuid::now_v7(),
            action: ActionKey::PlaceOrder,
            input: input(),
        };
        let json = serde_json::to_string(&vec![write.clone()]).unwrap();
        let back: Vec<PendingWrite> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, vec![write]);
    }
}
