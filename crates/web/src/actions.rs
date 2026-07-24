//! The two-step, acceptance-first write dispatcher (#17, ADR-20260720-015500) — the WRITE side of
//! the SDUI data layer (split 2/4 of #21).
//!
//! The platform's write model: a mutation NEVER returns the business result. It returns a
//! `MutationAcceptance` (the echoed technical envelope + `operationStatus: PENDING`), and the
//! OUTCOME IS A READ — poll `operationStatus(messageId)` until it leaves PENDING. Business
//! rejections are therefore not GraphQL errors: they arrive as status REJECTED with the stable
//! errors.yaml `errorCode`, and the client treats them as a normal, anticipated outcome
//! ([`ActionOutcome::Rejected`]) — completely distinct from a technical FAILED.
//!
//! The client mints the `messageId` (UUIDv7) and sends it in the mutation's `metadata`: that is the
//! WHOLE idempotency story. Re-sending the same logical intent with the same messageId (flaky V0
//! mobile radio, user double-tap, page reload mid-flight) journals nothing twice — the server
//! answers `duplicate: true` with the original's status, and polling converges on the original
//! outcome. Losing the messageId would turn every retry into a second order.
//!
//! [`dispatch`] is the ONLY public write entry point, keyed by the GENERATED [`ActionKey`]
//! allowlist — and it dispatches ONLY `kind: mutation` actions. `client`/`auth` kinds belong to
//! other layers (the renderer's client behaviours, the Supabase auth wrapper) and a `gap` kind
//! fails loudly ([`ActionError::GapAction`]) — never a silent no-op.

use std::time::Duration;

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::generated::data_layer::{ActionKey, ActionKind, ResolverKey};
use crate::graphql::{execute_resolver, ResolverError, Transport, TransportError};

/// Upper bound on `operationStatus` reads before giving up. 30 × [`POLL_INTERVAL`] ≈ a 30 s
/// ceiling: command handling is a single in-process journal write (typically sub-second), so a
/// PENDING that survives 30 s means something is genuinely wrong server-side — at that point the
/// honest UX is "still processing, retry safe" (the messageId makes the retry idempotent), not an
/// eternal spinner. An unbounded loop would also pin a mobile radio forever.
pub const POLL_MAX_ATTEMPTS: u32 = 30;

/// Spacing between `operationStatus` reads. 1 s is slow enough to be gentle on mobile radios and
/// the BFF (the poll is per in-flight command, not per screen), fast enough that the common
/// sub-second SUCCEEDED is seen on the first or second read. The push-based
/// `operationStatusChanged` subscription (split 3) will make polling the fallback, not the norm.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// The journaled operation lifecycle (`scalars.yaml#/OperationStatus`), parsed from the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStatus {
    Pending,
    Succeeded,
    Rejected,
    Failed,
}

impl OperationStatus {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "PENDING" => Some(Self::Pending),
            "SUCCEEDED" => Some(Self::Succeeded),
            "REJECTED" => Some(Self::Rejected),
            "FAILED" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Everything the write path can refuse or fail with. The first four variants are the dispatcher's
/// own allowlist enforcement — each non-dispatchable [`ActionKind`] gets its OWN variant so the
/// caller (and the log line) states exactly which contract was violated.
#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    /// `kind: client` — a renderer-local behaviour (navigate, share...); it never leaves the app.
    #[error("action `{0}` is client-side (kind `client`) — not a GraphQL mutation")]
    ClientSideAction(&'static str),
    /// `kind: auth` — a Supabase auth-provider call outside the domain write path.
    #[error("action `{0}` is an auth-provider call (kind `auth`) — not a GraphQL mutation")]
    AuthAction(&'static str),
    /// `kind: gap` — the UI wants a write the API does not model. Fail loudly with the spec's own
    /// note; the fix is a spec change, never a client workaround.
    #[error("action `{key}` is a declared gap — refusing to no-op: {note}")]
    GapAction { key: &'static str, note: &'static str },
    /// `kind: mutation` with no bound mutation — a generator invariant broken (defensive; the
    /// codegen validator makes this unrepresentable today).
    #[error("action `{0}` has kind `mutation` but no bound mutation — generated allowlist is inconsistent")]
    UnboundMutation(&'static str),
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// Polling reads go through `execute_resolver` — its failures surface unchanged.
    #[error(transparent)]
    Resolver(#[from] ResolverError),
    /// A 2xx acceptance whose shape is not the uniform `MutationAcceptance` contract.
    #[error("malformed MutationAcceptance: {0}")]
    MalformedAcceptance(String),
    /// The poll bound was reached with the operation still PENDING. NOT a failure verdict — the
    /// command may yet succeed server-side; the retry (same messageId) is idempotent and will see
    /// the real outcome.
    #[error("operation {message_id} unresolved after {attempts} status reads — giving up (same-messageId retry is safe)")]
    PollingExhausted { message_id: Uuid, attempts: u32 },
}

/// The terminal verdict of one dispatched action. `Rejected` vs `Failed` is a REAL distinction the
/// codebase leans on everywhere: REJECTED is an anticipated business invariant (errors.yaml code →
/// translated user message, normal UX flow), FAILED is a technical fault (retry/support territory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionOutcome {
    Succeeded {
        message_id: Uuid,
    },
    /// An anticipated business rejection. `error_code` is the stable errors.yaml code — required:
    /// a REJECTED without it is a server contract breach and surfaces as `MalformedAcceptance`.
    Rejected {
        message_id: Uuid,
        error_code: String,
        message: Option<String>,
    },
    /// A technical failure after acceptance (handler crash, infra). `error_code` may be absent.
    Failed {
        message_id: Uuid,
        error_code: Option<String>,
        message: Option<String>,
    },
}

/// The acceptance handle: proof the command is journaled, plus everything needed to resolve (or
/// retry) it. Persisting `message_id` across reloads until a terminal status is exactly how #12's
/// checkout continuity works — this struct is deliberately plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchHandle {
    /// The client-minted idempotency key of this logical intent.
    pub message_id: Uuid,
    /// True when the server had already journaled this messageId (an idempotent replay).
    pub duplicate: bool,
    /// The status echoed on the acceptance: PENDING on a first submission; a duplicate echoes the
    /// original's CURRENT status, which lets `resolve` skip polling entirely when it is already
    /// terminal-successful.
    pub status_at_acceptance: OperationStatus,
}

/// Dispatch an allowlisted action: the ONLY public write entry point of the crate.
///
/// Enforces the [`ActionKind`] contract (only `mutation` dispatches — distinct refusals for
/// `client`/`auth`/`gap`), mints the idempotency `messageId` (UUIDv7 — time-ordered like every id
/// the platform mints), and sends the convention-derived document
/// `mutation ($input: <Pascal>Input!, $metadata: MetadataInput)` selecting the UNIFORM
/// `MutationAcceptance` — the one selection set that IS honestly known, because every mutation
/// returns it by contract (api.yaml forbids per-operation payloads).
///
/// The returned [`DispatchHandle`] carries the messageId; await the verdict with
/// [`DispatchHandle::resolve`].
pub async fn dispatch(
    transport: &dyn Transport,
    key: ActionKey,
    input: Map<String, Value>,
) -> Result<DispatchHandle, ActionError> {
    dispatch_with_id(transport, key, input, Uuid::now_v7()).await
}

/// [`dispatch`] with a caller-supplied `message_id` — the SAME-messageId retry path (`pending.rs`,
/// #17): re-sending a persisted intent under its original id is what makes the retry idempotent
/// (the server answers `duplicate: true` and polling converges on the original outcome). Every
/// fresh dispatch goes through [`dispatch`], which mints here.
pub async fn dispatch_with_id(
    transport: &dyn Transport,
    key: ActionKey,
    input: Map<String, Value>,
    message_id: Uuid,
) -> Result<DispatchHandle, ActionError> {
    match key.kind() {
        ActionKind::Client => return Err(ActionError::ClientSideAction(key.as_str())),
        ActionKind::Auth => return Err(ActionError::AuthAction(key.as_str())),
        ActionKind::Gap => {
            return Err(ActionError::GapAction {
                key: key.as_str(),
                note: key.gap().unwrap_or("unbound action with no gap note"),
            })
        }
        ActionKind::Mutation => {}
    }
    let mutation = key.mutation().ok_or(ActionError::UnboundMutation(key.as_str()))?;
    // The SDL's own input-type name (#97): a mutation's input is named after its COMMAND
    // (`PlaceOrderInput`), which only coincides with the mutation name for most ops — read it,
    // never derive it. Absent for a `mutation` kind = the same broken-allowlist invariant as an
    // unbound mutation.
    let input_type = key.input_type().ok_or(ActionError::UnboundMutation(key.as_str()))?;

    let document = mutation_document(mutation, input_type);
    // Only messageId goes in metadata here: correlationId/causeId are server-computed defaults
    // (MetadataInput allows them, but this client has no causality chain to assert yet), and
    // sessionId/traceId travel as headers by contract — MetadataInput refuses them.
    let variables = json!({
        "input": Value::Object(input),
        "metadata": { "messageId": message_id },
    });

    let data = transport.execute(&document, variables).await?;
    let acceptance = data
        .get(mutation)
        .ok_or_else(|| ActionError::MalformedAcceptance(format!("data has no `{mutation}` field")))?;
    let status = acceptance
        .get("operationStatus")
        .and_then(Value::as_str)
        .and_then(OperationStatus::parse)
        .ok_or_else(|| ActionError::MalformedAcceptance("missing/unknown operationStatus".into()))?;
    let duplicate = acceptance.get("duplicate").and_then(Value::as_bool).unwrap_or(false);

    Ok(DispatchHandle { message_id, duplicate, status_at_acceptance: status })
}

impl DispatchHandle {
    /// Resolve the terminal outcome with the production poll bounds ([`POLL_MAX_ATTEMPTS`] ×
    /// [`POLL_INTERVAL`]).
    pub async fn resolve(&self, transport: &dyn Transport) -> Result<ActionOutcome, ActionError> {
        self.resolve_with(transport, POLL_MAX_ATTEMPTS, POLL_INTERVAL).await
    }

    /// Resolve with explicit bounds (tests use `Duration::ZERO`; production goes through
    /// [`DispatchHandle::resolve`]). `max_attempts` counts `operationStatus` READS — the first is
    /// immediate (the common sub-second SUCCEEDED needs no wait), each further read is preceded by
    /// `interval`. Reaching the bound returns [`ActionError::PollingExhausted`], never loops on.
    pub async fn resolve_with(
        &self,
        transport: &dyn Transport,
        max_attempts: u32,
        interval: Duration,
    ) -> Result<ActionOutcome, ActionError> {
        // A duplicate acceptance echoes the original's CURRENT status: already-succeeded needs no
        // read at all. Terminal-unhappy echoes still poll once — the acceptance has no
        // errorCode/message, the Operation read does.
        if self.status_at_acceptance == OperationStatus::Succeeded {
            return Ok(ActionOutcome::Succeeded { message_id: self.message_id });
        }

        let mut poll_vars = Map::new();
        poll_vars.insert("messageId".into(), json!(self.message_id));

        for attempt in 1..=max_attempts {
            if attempt > 1 && !interval.is_zero() {
                sleep(interval).await;
            }
            let operation =
                execute_resolver(transport, ResolverKey::OperationStatusByMessage, poll_vars.clone())
                    .await?;
            // `operationStatus` resolves null to strangers AND during the tiny window before the
            // journal row is readable — both are "no verdict yet", so keep polling (the bound
            // protects us from polling a row we will never be allowed to see).
            if operation.is_null() {
                continue;
            }
            let status = operation
                .get("status")
                .and_then(Value::as_str)
                .and_then(OperationStatus::parse)
                .ok_or_else(|| {
                    ActionError::MalformedAcceptance("Operation without a valid status".into())
                })?;
            let message =
                operation.get("message").and_then(Value::as_str).map(str::to_string);
            match status {
                OperationStatus::Pending => continue,
                OperationStatus::Succeeded => {
                    return Ok(ActionOutcome::Succeeded { message_id: self.message_id })
                }
                OperationStatus::Rejected => {
                    // The stable errors.yaml code is the rejection contract (P-10) — its absence
                    // is a server bug we surface, not paper over.
                    let error_code = operation
                        .get("errorCode")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .ok_or_else(|| {
                            ActionError::MalformedAcceptance("REJECTED without errorCode".into())
                        })?;
                    return Ok(ActionOutcome::Rejected {
                        message_id: self.message_id,
                        error_code,
                        message,
                    });
                }
                OperationStatus::Failed => {
                    return Ok(ActionOutcome::Failed {
                        message_id: self.message_id,
                        error_code: operation
                            .get("errorCode")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        message,
                    });
                }
            }
        }
        Err(ActionError::PollingExhausted { message_id: self.message_id, attempts: max_attempts })
    }
}

/// The mutation document: the GENERATED input-type name (#97) + the uniform `MutationAcceptance`
/// selection — the ONE selection set known by contract for every mutation.
fn mutation_document(mutation: &str, input_type: &str) -> String {
    format!(
        "mutation Dispatch($input: {input_type}!, $metadata: MetadataInput) \
{{ {mutation}(input: $input, metadata: $metadata) \
{{ messageId correlationId causeId sessionId traceId operationStatus duplicate }} }}"
    )
}

/// Await `d` on whichever timer the target has: tokio natively, the browser's setTimeout (via
/// gloo) on wasm32. Zero-duration sleeps return immediately (the test path). `pub(crate)`: the
/// checkout intent poll (`checkout.rs`) paces on the same timer.
pub(crate) async fn sleep(d: Duration) {
    if d.is_zero() {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(d).await;
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(d.as_millis().min(u32::MAX as u128) as u32).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql::test_support::FakeTransport;

    /// Canned acceptance payload for `addCartLine`.
    fn acceptance(status: &str, duplicate: bool) -> Value {
        json!({ "addCartLine": {
            "messageId": "00000000-0000-7000-8000-000000000000",
            "correlationId": "00000000-0000-7000-8000-000000000000",
            "causeId": null, "sessionId": null, "traceId": null,
            "operationStatus": status, "duplicate": duplicate,
        }})
    }

    /// Canned `operationStatus` poll answer.
    fn operation(status: &str, error_code: Option<&str>, message: Option<&str>) -> Value {
        json!({ "operationStatus": {
            "messageId": "00000000-0000-7000-8000-000000000000",
            "correlationId": "00000000-0000-7000-8000-000000000000",
            "status": status, "errorCode": error_code, "message": message,
            "occurredAt": "2026-07-23T12:00:00Z",
        }})
    }

    #[tokio::test]
    async fn non_mutation_actions_are_refused_with_distinct_variants() {
        let fake = FakeTransport::scripted(vec![]);
        // client / auth / gap each hit their OWN variant — and none reaches the transport.
        assert!(matches!(
            dispatch(&fake, ActionKey::Navigate, Map::new()).await.unwrap_err(),
            ActionError::ClientSideAction("navigate")
        ));
        assert!(matches!(
            dispatch(&fake, ActionKey::SignOut, Map::new()).await.unwrap_err(),
            ActionError::AuthAction("sign_out")
        ));
        match dispatch(&fake, ActionKey::ApplyPromoCode, Map::new()).await.unwrap_err() {
            ActionError::GapAction { key, note } => {
                assert_eq!(key, "apply_promo_code");
                assert!(note.contains("applyPromoCode"), "gap note should be the spec's: {note}");
            }
            other => panic!("expected GapAction, got {other:?}"),
        }
        assert_eq!(fake.call_count(), 0, "refusals must fail closed, not reach the transport");
    }

    #[tokio::test]
    async fn dispatch_mints_a_v7_message_id_and_sends_it_in_metadata() {
        let fake = FakeTransport::scripted(vec![Ok(acceptance("PENDING", false))]);
        let mut input = Map::new();
        input.insert("offerId".into(), json!("offer-1"));
        let handle = dispatch(&fake, ActionKey::AddToCart, input).await.unwrap();

        assert_eq!(handle.message_id.get_version_num(), 7);
        assert!(!handle.duplicate);
        assert_eq!(handle.status_at_acceptance, OperationStatus::Pending);

        let (document, variables) = fake.call(0);
        // Convention-derived document + the uniform MutationAcceptance selection.
        assert!(document.contains("$input: AddCartLineInput!"), "{document}");
        assert!(document.contains("addCartLine(input: $input, metadata: $metadata)"), "{document}");
        assert!(document.contains("operationStatus duplicate"), "{document}");
        // The minted messageId travels in metadata — the idempotency contract.
        assert_eq!(variables["metadata"]["messageId"], json!(handle.message_id.to_string()));
        assert_eq!(variables["input"]["offerId"], json!("offer-1"));
    }

    #[tokio::test]
    async fn two_step_happy_path_pending_then_poll_to_succeeded() {
        let fake = FakeTransport::scripted(vec![
            Ok(acceptance("PENDING", false)),
            Ok(operation("PENDING", None, None)),
            Ok(operation("SUCCEEDED", None, None)),
        ]);
        let handle = dispatch(&fake, ActionKey::AddToCart, Map::new()).await.unwrap();
        let outcome = handle.resolve_with(&fake, 5, Duration::ZERO).await.unwrap();
        assert_eq!(outcome, ActionOutcome::Succeeded { message_id: handle.message_id });
        // 1 mutation + 2 status reads, and the poll queried OUR messageId.
        assert_eq!(fake.call_count(), 3);
        assert_eq!(fake.call(1).1["input"]["messageId"], json!(handle.message_id.to_string()));
    }

    #[tokio::test]
    async fn rejected_surfaces_its_error_code_distinctly_from_failed() {
        let fake = FakeTransport::scripted(vec![
            Ok(acceptance("PENDING", false)),
            Ok(operation("REJECTED", Some("RestaurantPaused"), Some("Restaurant is paused"))),
        ]);
        let handle = dispatch(&fake, ActionKey::AddToCart, Map::new()).await.unwrap();
        match handle.resolve_with(&fake, 5, Duration::ZERO).await.unwrap() {
            ActionOutcome::Rejected { error_code, message, .. } => {
                assert_eq!(error_code, "RestaurantPaused");
                assert_eq!(message.as_deref(), Some("Restaurant is paused"));
            }
            other => panic!("a business rejection must be Rejected, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn failed_is_the_technical_outcome_not_a_rejection() {
        let fake = FakeTransport::scripted(vec![
            Ok(acceptance("PENDING", false)),
            Ok(operation("FAILED", None, Some("handler panicked"))),
        ]);
        let handle = dispatch(&fake, ActionKey::AddToCart, Map::new()).await.unwrap();
        match handle.resolve_with(&fake, 5, Duration::ZERO).await.unwrap() {
            ActionOutcome::Failed { error_code, message, .. } => {
                assert_eq!(error_code, None);
                assert_eq!(message.as_deref(), Some("handler panicked"));
            }
            other => panic!("a technical failure must be Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn polling_gives_up_at_the_bound_instead_of_looping_forever() {
        let fake = FakeTransport::scripted(vec![
            Ok(acceptance("PENDING", false)),
            Ok(operation("PENDING", None, None)),
            Ok(operation("PENDING", None, None)),
            Ok(operation("PENDING", None, None)),
        ]);
        let handle = dispatch(&fake, ActionKey::AddToCart, Map::new()).await.unwrap();
        let err = handle.resolve_with(&fake, 3, Duration::ZERO).await.unwrap_err();
        assert!(matches!(err, ActionError::PollingExhausted { attempts: 3, .. }));
        // Exactly the bound: 1 mutation + 3 status reads, not one more.
        assert_eq!(fake.call_count(), 4);
    }

    #[tokio::test]
    async fn duplicate_already_succeeded_acceptance_resolves_without_polling() {
        // A retry of an already-completed intent: the acceptance echoes SUCCEEDED, so resolving
        // needs zero reads — the idempotency payoff.
        let fake = FakeTransport::scripted(vec![Ok(acceptance("SUCCEEDED", true))]);
        let handle = dispatch(&fake, ActionKey::AddToCart, Map::new()).await.unwrap();
        assert!(handle.duplicate);
        let outcome = handle.resolve_with(&fake, 5, Duration::ZERO).await.unwrap();
        assert_eq!(outcome, ActionOutcome::Succeeded { message_id: handle.message_id });
        assert_eq!(fake.call_count(), 1, "terminal-successful acceptance must not poll");
    }
}
