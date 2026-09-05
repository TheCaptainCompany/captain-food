//! Composition support for the `graphql-{scope}` SUBGRAPH bins (#385 API-tier wiring,
//! PROP-20260807-174246 D8).
//!
//! A subgraph bin is the monolith's GraphQL surface RESTRICTED to one domain: the same generated
//! type layer, the same resolvers over the same adapters ([`crate::build_graphql_di`] — one
//! composition, no logic fork), the same role-as-path auth boundary (ADR-0006/ADR-0047), plus the
//! scope slice (`graphql::scope_slice`) that rejects any top-level field owned by another scope.
//! The GRANT-scoped views role (#360) later adds the storage wall under this serving wall.
//!
//! HONEST LIMITS (the recorded #385 trade-offs, same as the spine):
//! - The operation-status and event buses are process-local: completions delivered by an actor
//!   bin reach this process's POLL reads (`operationStatus` reads the mailbox row) but not its
//!   push subscribers. Cross-process fan-out is the recorded follow-up.
//! - Mutations ENQUEUE on the shared mailbox (acceptance-first); the in-transaction `pg_notify`
//!   raised by the mailbox INSERT wakes the owning actor bin's fleet — delivery needs no worker
//!   in this process.

use std::sync::Arc;

use axum::{middleware, Extension, Router};
use infrastructure::EventBus;
use sqlx::PgPool;

/// What a subgraph bin's generated Config supplies (its scope-filtered key subset, #374 Q4).
pub struct SubgraphSettings {
    /// The owning scope — the slice this bin serves (e.g. `"ordering"`).
    pub scope: &'static str,
    /// The bin name, for posture logging (e.g. `"graphql-ordering"`).
    pub bin: &'static str,
    pub supabase_jwks_url: String,
    pub supabase_url: String,
}

/// Build the axum app of one subgraph bin: `/{role}/graphql` (+ `/{role}/voyager`) exactly as the
/// monolith serves them — per-role auth at the path boundary, per-field ACL + role-filtered
/// introspection in the schema — restricted to `settings.scope` by the scope-slice extension.
pub async fn subgraph_app(pool: PgPool, settings: SubgraphSettings) -> Router {
    // Process-local buses (the recorded cross-process push gap): constructed so the schema always
    // carries them — poll paths are authoritative in the bin topology.
    let event_bus = EventBus::default();
    let status_bus = actor_client::OperationStatusBus::default();
    // Enqueue→worker nudges are in-process wakes; no worker runs here, but the mailbox INSERT's
    // in-transaction pg_notify wakes the owning actor bin's fleet (ADR-20260802-200416).
    let nudges = {
        let mut n = infrastructure::persistence::mailbox_store::MailboxNudges::default();
        for (actor_type, _) in infrastructure::generated::command_router::ACTOR_MAILBOXES {
            n.register(actor_type);
        }
        Arc::new(n)
    };
    // #516: the per-surface gateway bins get the send guards too, over the SAME shared counter — the
    // whole point of the counter being in Postgres is that every replica and every bin counts into one
    // row. Their identity ACL then sheds a doomed OTP request with a typed reason; the authoritative
    // wall stays the `/auth/sms-hook` route, which the monolith still hosts.
    let (config, _) = crate::generated::config::Config::resolve();
    let sms_guard = Some(crate::sms_send_guard(&pool, &config));
    // #639 part C step 4-ii (ADR-20260904-124600 §4): the SAME `SUPPORT_CONTACT` parse the
    // monolith's `router()` does — a subgraph bin serves the same resolvers over the same
    // configuration, no logic fork (#385 D8).
    let support_contact: Option<domain::generated::scalars::EmailAddress> =
        Some(config.support_contact.trim())
            .filter(|s| !s.is_empty())
            .map(|s| domain::generated::scalars::EmailAddress(s.to_string()));
    let di = crate::build_graphql_di(
        &pool,
        &event_bus,
        &status_bus,
        &nudges,
        sms_guard,
        crate::graphql::service_clock::ServiceWindowHorizon::from_seconds(
            config.service_window_validity_horizon_seconds,
        ),
        support_contact,
        config.run_rider_restriction_door,
    );
    // IDENT-1 Phase A (#641, ADR-20260818-004646): the SAME gate-then-stabilize choice the
    // monolith makes, over the SAME `customers` repository -- a subgraph bin serves the identical
    // resolvers over the same adapters (#385 D8), no logic fork.
    let customer_identity_source = if config.resolve_customer_identity_from_postgres {
        crate::auth::CustomerIdentitySource::Postgres(std::sync::Arc::new(
            crate::auth::PgCustomerIdentity::new(di.read.customers.clone()),
        ))
    } else {
        crate::auth::CustomerIdentitySource::Claim
    };
    // The RIDER seam has no gate (#639 part C step 2b): Postgres, over the `Rider` projection's
    // `auth_ref` bridge, in every bin that mounts `/rider/graphql` -- the same adapter the monolith
    // wires, no logic fork.
    let identity_sources = crate::auth::IdentitySources {
        customer: customer_identity_source,
        rider: crate::auth::RiderIdentitySource::new(std::sync::Arc::new(
            crate::auth::PgRiderIdentity::new(std::sync::Arc::new(
                infrastructure::PgRiderRepository::new(pool.clone()),
            )),
        )),
    };
    let schema = crate::graphql_schema::build_schema_for_scope(
        Some(di.read),
        Some(di.write),
        Some(event_bus),
        Some(settings.scope),
    );
    crate::graphql::routes::graphql_routes(schema, di.tenant_lookup, identity_sources)
        // API auth (ADR-0047): the same Supabase-JWT verifier the monolith layers — the subgraph
        // IS the schema boundary, so authn/authz live here, never in the (stateless) gateway.
        .layer(Extension(crate::auth::AuthContext::from_config(
            settings.supabase_jwks_url,
            settings.supabase_url,
        )))
        // Same response identity/timing headers as the monolith (ADR-20260721-175411).
        .layer(middleware::from_fn(crate::response_timing))
}
