//! Auth-session parking (#112, PROP-20260724-150500) — the bridge between server-side OTP
//! verification and the client's httpOnly cookie.
//!
//! The identity provider issues its session (access/refresh JWTs) INSIDE the async
//! VerifyPhone/ConfirmEmailVerification handlers — but the client only holds the acceptance
//! `messageId`. The handlers PARK the session here (keyed by that messageId, owned by the
//! journaling anonymous `session_id`); the BFF's `POST /auth/session` CLAIMS it — a single-read
//! exchange that deletes the row — and answers with the cookie. Ownership mirrors
//! `operationStatus`: the claimer's `X-SESSION-ID` must equal the one that journaled the command.
//!
//! The port speaks PLAINTEXT structs; encryption at rest (AES-256-GCM under `AUTH_SESSION_KEY`)
//! is the Pg adapter's concern — same layering as every credential store in this repo. Parking
//! failures never fail the verification: the identity fact stands, the user can re-request an OTP;
//! the handler logs and continues.

use async_trait::async_trait;
use domain::shared::errors::DomainError;
use uuid::Uuid;

/// A provider session awaiting cookie pickup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParkedAuthSession {
    /// The acceptance handle of the verifying command (`actor.cause_id`).
    pub message_id: Uuid,
    /// The anonymous session that journaled it — the claim-ownership key (None on channels
    /// without one; the claim must then also present none).
    pub session_id: Option<Uuid>,
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Access-token lifetime in seconds, as the provider reported it.
    pub expires_in: Option<i64>,
}

#[async_trait]
pub trait AuthSessionStore: Send + Sync {
    /// Park a session for pickup (idempotent per message_id — a handler re-run replaces).
    async fn park(&self, session: ParkedAuthSession) -> Result<(), DomainError>;

    /// Claim-and-delete: returns the session iff the row exists, is unexpired, and its owning
    /// `session_id` equals the presented one (both-`None` matches — a channel without anonymous
    /// sessions). A mismatch returns `None` indistinguishably from absence — no oracle.
    async fn claim(
        &self,
        message_id: Uuid,
        session_id: Option<Uuid>,
    ) -> Result<Option<ParkedAuthSession>, DomainError>;
}

/// The fail-closed default (no DB or no `AUTH_SESSION_KEY`): parking is a no-op SUCCESS — a
/// deployment without session storage must not fail verification — and claiming always yields
/// `None`, so `POST /auth/session` answers 404 (no cookie) rather than leaking. Anonymous still
/// works; authenticated features just stay unavailable until storage is configured.
#[derive(Default)]
pub struct NoopAuthSessionStore;

#[async_trait]
impl AuthSessionStore for NoopAuthSessionStore {
    async fn park(&self, _session: ParkedAuthSession) -> Result<(), DomainError> {
        Ok(())
    }
    async fn claim(
        &self,
        _message_id: Uuid,
        _session_id: Option<Uuid>,
    ) -> Result<Option<ParkedAuthSession>, DomainError> {
        Ok(None)
    }
}

/// In-memory double for tests and the behaviour runtime.
pub mod mem {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct MemAuthSessionStore(Mutex<Vec<ParkedAuthSession>>);

    impl MemAuthSessionStore {
        pub fn parked(&self) -> Vec<ParkedAuthSession> {
            self.0.lock().expect("auth session mutex").clone()
        }
    }

    #[async_trait]
    impl AuthSessionStore for MemAuthSessionStore {
        async fn park(&self, session: ParkedAuthSession) -> Result<(), DomainError> {
            let mut rows = self.0.lock().expect("auth session mutex");
            rows.retain(|r| r.message_id != session.message_id);
            rows.push(session);
            Ok(())
        }

        async fn claim(
            &self,
            message_id: Uuid,
            session_id: Option<Uuid>,
        ) -> Result<Option<ParkedAuthSession>, DomainError> {
            let mut rows = self.0.lock().expect("auth session mutex");
            let hit = rows
                .iter()
                .position(|r| r.message_id == message_id && r.session_id == session_id);
            Ok(hit.map(|i| rows.remove(i)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mem::MemAuthSessionStore;
    use super::*;

    fn parked(message_id: Uuid, session_id: Option<Uuid>) -> ParkedAuthSession {
        ParkedAuthSession {
            message_id,
            session_id,
            access_token: "jwt".into(),
            refresh_token: Some("refresh".into()),
            expires_in: Some(3600),
        }
    }

    #[tokio::test]
    async fn claim_is_single_read_and_ownership_scoped() {
        let store = MemAuthSessionStore::default();
        let mid = Uuid::now_v7();
        let owner = Uuid::now_v7();
        store.park(parked(mid, Some(owner))).await.unwrap();

        // Wrong owner → None (no oracle), and the row survives for the true owner.
        assert!(store.claim(mid, Some(Uuid::now_v7())).await.unwrap().is_none());
        assert!(store.claim(mid, None).await.unwrap().is_none());
        // Right owner → the session, once.
        let got = store.claim(mid, Some(owner)).await.unwrap().expect("owner claims");
        assert_eq!(got.access_token, "jwt");
        // Single-read: the second claim finds nothing.
        assert!(store.claim(mid, Some(owner)).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn noop_store_parks_without_error_and_never_yields() {
        let store = NoopAuthSessionStore;
        store.park(parked(Uuid::now_v7(), None)).await.unwrap();
        assert!(store.claim(Uuid::now_v7(), None).await.unwrap().is_none());
    }
}
