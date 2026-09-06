//! The one-shot `bootstrap-platform-admin` subcommand (#639 part C step 6-v,
//! ADR-20260905-223957 §3): the FIRST admin is a recorded ACT through the ordinary mailbox
//! enqueue path -- never a row, never SQL, never a data migration. The smallest honest home for
//! this is a `server` bin subcommand (the `bo-admin`/per-actor bins are GENERATED, per-deployable
//! skeletons with no room for a hand-written one-shot script; `worker-erasure`/`worker-retention`
//! are likewise GENERATED cron shapes, one bounded pass per Job, not an operator-invoked command):
//! the monolith already composes the Postgres pool + the typed mailbox door every ordinary write
//! goes through, so this subcommand reuses that composition rather than inventing a second one.
//!
//! Invocation: `server bootstrap-platform-admin` (any DATABASE_URL-carrying environment). Reads
//! `PLATFORM_BOOTSTRAP_ADMIN_SUBJECT` (a declared SECRET, `specs/common/configuration.yaml`,
//! `required: []` -- never required at ordinary boot; the founder provisions it only when this is
//! actually run, ADMIN-DOOR-PRECONDITIONS item 1). Mints the DETERMINISTIC `platformMembershipId`
//! via [`application::commands::platform_membership_id_for`] (round 2, R2-5: the ONE place that
//! formula lives, so this bootstrap and the handler's own `PlatformMembershipIdMismatch` check can
//! never drift apart), so running it twice targets the SAME stream and the fold's own idempotent
//! `NoChange` is what makes a second run inert -- never a script-level "already ran" check, which
//! would be a second, competing source of truth. Dispatches
//! `GrantPlatformAccess` (`basis: CAPTAIN_ONBOARDING`) through the SAME `PlatformMembershipClient`
//! the GraphQL door uses, with an actor envelope naming the bootstrap itself as the acting
//! principal (`user_type: "ADMIN"`, a fixed deterministic `user_id` -- never the subject being
//! granted, which would misattribute the act as self-performed). Gated by
//! `RUN_PLATFORM_ACCESS_GRANT` like every grant -- OFF, the handler refuses before the store is
//! touched, exactly as it does for the GraphQL door.
//!
//! Runs inside the ordinary OTLP-wired server process (observability CATCH: never a bare script
//! outside telemetry) and leaves the Art. 5(2) accountability artifact -- who (the bootstrap
//! actor's fixed id), when (this log line's timestamp), basis (`CAPTAIN_ONBOARDING`), authority
//! (ADMIN-DOOR-PRECONDITIONS item 1) -- as a structured `tracing::info!` line, the operator's own
//! correlation id riding the envelope's `correlation_id`.

use std::sync::Arc;

use actor_client::mailbox::{Envelope, Mailbox};
use actor_client::EnqueueOutcome;
use domain::generated::commands::GrantPlatformAccess;
use domain::generated::scalars::{AuthSubject, PlatformAccessBasis, PlatformMembershipId};

/// Fixed UUIDv5 namespace for the ENVELOPE-level ids this subcommand mints itself
/// (`message_id`, `bootstrap_system_user_id`) -- NEVER change it. The `platformMembershipId`
/// ITSELF is no longer derived here (round 2, R2-5): it is
/// [`application::commands::platform_membership_id_for`], the ONE place that formula lives, so the
/// handler's own `PlatformMembershipIdMismatch` check and this bootstrap can never silently drift
/// apart. This namespace constant happens to share its literal with that function's namespace
/// (both are stable, both predate the split) but is no longer the source of truth for the grant id.
fn bootstrap_namespace() -> uuid::Uuid {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        b"https://captain.food/integrations/bootstrap-platform-admin",
    )
}

/// The REQUIRED `platformMembershipId` for one `authSubject` -- re-exported from
/// [`application::commands::platform_membership_id_for`], the ONE place the formula lives (round
/// 2, R2-5: "Namespace constant: ONE place, named"). `pub` so a DB-gated integration test can
/// compute the SAME value a `then:`/assertion must match.
pub fn platform_membership_id_for(auth_subject: &str) -> PlatformMembershipId {
    application::commands::platform_membership_id_for(auth_subject)
}

/// The fixed system principal this subcommand acts as -- deterministic, so every bootstrap run
/// (across every environment) is attributable to the SAME `domain_events.user_id` (ADR-0041), and
/// distinct from the being-granted subject's own id (attributing a grant to its own beneficiary
/// would be dishonest provenance). The `sirene_system_user_id` precedent.
fn bootstrap_system_user_id() -> uuid::Uuid {
    uuid::Uuid::new_v5(&bootstrap_namespace(), b"system:bootstrap-platform-admin")
}

/// The WORKER-channel [`Envelope`] for one bootstrap dispatch: `message_id` is deterministic over
/// the target `authSubject`, so a re-run against the SAME subject replays the SAME mailbox
/// identity and the door's own dedupe (`EnqueueOutcome::Deduplicated`) absorbs it before the
/// worker is even asked to fold anything.
fn bootstrap_envelope(auth_subject: &str) -> Envelope {
    let correlation_id = uuid::Uuid::new_v4();
    Envelope {
        message_id: uuid::Uuid::new_v5(&bootstrap_namespace(), format!("grant:{auth_subject}").as_bytes()),
        correlation_id,
        cause_id: None,
        session_id: None,
        trace_id: None,
        user_id: Some(bootstrap_system_user_id()),
        user_type: "ADMIN".to_string(),
        channel: "WORKER".to_string(),
    }
}

/// Run the subcommand: resolve the declared secret, mint the deterministic id, dispatch the
/// command through the ordinary mailbox door. Returns the process exit code (sysexits.h-style):
/// `0` success, `78` (EX_CONFIG) the secret is unset or `DATABASE_URL` is empty -- a configuration
/// precondition, not a crash -- `1` the dispatch itself failed (a database/mailbox error).
pub async fn run(config: &crate::generated::config::Config) -> i32 {
    let Some(auth_subject) = config
        .platform_bootstrap_admin_subject
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        tracing::error!(
            "bootstrap-platform-admin: PLATFORM_BOOTSTRAP_ADMIN_SUBJECT is unset -- refusing to \
             run. The founder provisions this secret only when the bootstrap is actually invoked \
             (ADMIN-DOOR-PRECONDITIONS item 1); the ordinary server boot never reads this key."
        );
        return 78;
    };
    if config.database_url.trim().is_empty() {
        tracing::error!(
            "bootstrap-platform-admin: DATABASE_URL is unset -- there is no mailbox to dispatch \
             through."
        );
        return 78;
    }
    dispatch(&config.database_url, auth_subject).await
}

/// The subcommand's ACTUAL logic, taking its two inputs directly (never the whole generated
/// `Config`) -- so a test can exercise the exact dispatch a real invocation performs without
/// resolving all ~200 other declared keys `Config::resolve()` would otherwise demand. `run` above
/// is a thin wrapper extracting these two fields and handling their absence.
pub async fn dispatch(database_url: &str, auth_subject: &str) -> i32 {
    let pool = match sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(database_url)
        .await
    {
        Ok(pool) => pool,
        Err(e) => {
            tracing::error!(error = %e, "bootstrap-platform-admin: could not connect to DATABASE_URL");
            return 1;
        }
    };

    let mailbox: Arc<dyn Mailbox> =
        Arc::new(infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone()));
    let platform_membership_id = platform_membership_id_for(auth_subject);
    let client = client_platform_membership::PlatformMembershipClient::new(
        mailbox,
        platform_membership_id.0,
    );
    let cmd = GrantPlatformAccess {
        platform_membership_id,
        auth_subject: AuthSubject(auth_subject.to_string()),
        basis: PlatformAccessBasis::CAPTAIN_ONBOARDING,
    };
    let env = bootstrap_envelope(auth_subject);
    let correlation_id = env.correlation_id;
    match client.send(cmd, env).await {
        Ok(EnqueueOutcome::Enqueued) => {
            // The Art. 5(2) accountability artifact: who, when (this line's own timestamp), basis,
            // authority. `auth_subject` is the person's OWN login credential, not a secret in the
            // sense DATABASE_URL/PLATFORM_BOOTSTRAP_ADMIN_SUBJECT are -- it is exactly what
            // `PlatformAccessGranted` already records in `domain_events`, so logging it here adds
            // no new exposure.
            tracing::info!(
                auth_subject,
                platform_membership_id = %platform_membership_id.0,
                basis = "CAPTAIN_ONBOARDING",
                authority = "ADMIN-DOOR-PRECONDITIONS item 1",
                correlation_id = %correlation_id,
                "bootstrap-platform-admin: GrantPlatformAccess enqueued (acceptance-first, PENDING)"
            );
            0
        }
        Ok(EnqueueOutcome::Deduplicated(status)) => {
            tracing::info!(
                auth_subject,
                platform_membership_id = %platform_membership_id.0,
                status = ?status,
                correlation_id = %correlation_id,
                "bootstrap-platform-admin: already dispatched (idempotent replay) -- running it \
                 twice appends one fact"
            );
            0
        }
        Ok(EnqueueOutcome::PayloadConflict(status)) => {
            // Unreachable in practice (the envelope + payload are both pure functions of
            // `auth_subject`), but named rather than absorbed by a wildcard.
            tracing::error!(
                auth_subject,
                status = ?status,
                "bootstrap-platform-admin: a DIFFERENT payload already occupies this message id -- refusing"
            );
            1
        }
        Err(e) => {
            tracing::error!(error = %e, auth_subject, "bootstrap-platform-admin: dispatch failed");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round 2, R2-5: "Namespace constant: ONE place, named" -- this bootstrap's re-exported
    /// [`platform_membership_id_for`] must be BYTE-IDENTICAL to
    /// [`application::commands::platform_membership_id_for`], the ONE place the formula actually
    /// lives. RED-FIRST: before this round, the bootstrap held its OWN copy of the derivation
    /// (`bootstrap_namespace()` feeding a locally-defined `platform_membership_id_for`) -- if
    /// anyone reintroduces a second copy here with even a byte of drift (a different namespace
    /// literal, a different input encoding), the handler's `PlatformMembershipIdMismatch` check
    /// would refuse EVERY bootstrap-dispatched grant, since the two derivations would disagree.
    /// This test is the mechanical proof that cannot happen while there is exactly one function.
    #[test]
    fn platform_membership_id_for_matches_the_one_true_derivation() {
        for subject in ["auth-admin-1", "already-granted-admin", "a3f1c8de-0b2e-4f77-9a11-0c4d2e8b7f10"] {
            assert_eq!(
                platform_membership_id_for(subject),
                application::commands::platform_membership_id_for(subject),
                "the bootstrap's platform_membership_id_for must be the SAME derivation as the \
                 handler's, for subject '{subject}' -- ONE place, named"
            );
        }
    }

    /// The derivation is a pure function of `authSubject` ONLY: two DIFFERENT subjects must never
    /// collide (or the handler's `PlatformMembershipIdMismatch` check and the bridge's
    /// `auth_subject UNIQUE` backstop would both be defending against a false positive).
    #[test]
    fn different_subjects_derive_different_ids() {
        assert_ne!(
            platform_membership_id_for("auth-admin-1"),
            platform_membership_id_for("auth-admin-2"),
        );
    }

    /// Determinism is the whole point: running the bootstrap twice against the SAME subject must
    /// target the SAME stream, or "running it twice appends one fact" could never hold.
    #[test]
    fn the_same_subject_always_derives_the_same_id() {
        assert_eq!(
            platform_membership_id_for("auth-admin-1"),
            platform_membership_id_for("auth-admin-1"),
        );
    }
}
