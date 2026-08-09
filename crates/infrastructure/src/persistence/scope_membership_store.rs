//! The `scopemembership` table (#144, PROP-20260725-185140 §3.4) — the single index every read-side
//! authorization question resolves against.
//!
//! Three operations, and the asymmetry between them is the whole safety story:
//!   * [`grant`]   — idempotent upsert, keyed by the UUIDv5-derived `membership_id`;
//!   * [`revoke_role`] — drops every principal of one role on one scope;
//!   * [`is_member`]   — the guard's read: one primary-key `EXISTS`, no joins, ever.
//!
//! A MISSING row denies (visible, safe). A STALE row grants (a silent breach). So `revoke_role`
//! deletes broadly and `grant` writes narrowly — never the other way round.
//!
//! Column conventions (ADR-20260728): `scope_type` and `principal_type` are TEXT (the variant name
//! verbatim, [`crate::persistence::enum_sql`]), self-describing without a lookup join.

use application::projectors::scope_membership::membership_id;
use application::queries::{ReadScope, ScopeMembershipRepository};
use domain::generated::scalars::{ScopeType, UserType};
use domain::shared::errors::DomainError;
use sqlx::PgPool;
use uuid::Uuid;

use super::db_err;
use super::enum_sql::EnumText;

/// Record one membership. Idempotent by construction: `membership_id` is derived from the natural
/// key, so replaying the same grant (a projection replay, a redelivered event) hits the same row —
/// and `ON CONFLICT DO NOTHING` writes nothing at all then, preserving `granted_at`/`created_at` and
/// producing no dead tuple per replayed event (the row is immutable once written; revocation is
/// deletion, never an update).
pub async fn grant(
    conn: &mut sqlx::PgConnection,
    scope_type: ScopeType,
    scope_id: Uuid,
    principal_type: UserType,
    principal_id: Uuid,
    granted_at: chrono::DateTime<chrono::Utc>,
) -> Result<(), DomainError> {
    let id = membership_id(scope_type, scope_id, principal_type, principal_id);
    sqlx::query(
        "INSERT INTO scopemembership \
           (membership_id, scope_type, scope_id, principal_type, principal_id, granted_at, created_at, updated_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$6,$6) \
         ON CONFLICT (membership_id) DO NOTHING",
    )
    .bind(id)
    .bind(scope_type.to_text())
    .bind(scope_id)
    .bind(principal_type.to_text())
    .bind(principal_id)
    .bind(granted_at)
    .execute(&mut *conn)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Drop EVERY principal holding `principal_type` on this scope.
///
/// Deliberately not "revoke this one rider": if a reassignment ever left two rider rows on one order,
/// a targeted delete would strip one and leave the other holding access — the stale-grant breach. The
/// broad delete is safe because the replacement grant is re-applied by the very next event.
pub async fn revoke_role(
    conn: &mut sqlx::PgConnection,
    scope_type: ScopeType,
    scope_id: Uuid,
    principal_type: UserType,
) -> Result<u64, DomainError> {
    let deleted = sqlx::query(
        "DELETE FROM scopemembership \
          WHERE scope_type = $1 AND scope_id = $2 AND principal_type = $3",
    )
    .bind(scope_type.to_text())
    .bind(scope_id)
    .bind(principal_type.to_text())
    .execute(&mut *conn)
    .await
    .map_err(db_err)?
    .rows_affected();
    Ok(deleted)
}

/// The guard's question, for every role and every surface: may this principal see this instance?
///
/// One primary-key lookup — the id is derived in-process from the four parts, so this never scans,
/// never joins, and costs the same whatever the scope type is.
pub async fn is_member(
    pool: &PgPool,
    scope_type: ScopeType,
    scope_id: Uuid,
    principal_type: UserType,
    principal_id: Uuid,
) -> Result<bool, DomainError> {
    let id = membership_id(scope_type, scope_id, principal_type, principal_id);
    let found: Option<(bool,)> =
        sqlx::query_as("SELECT true FROM scopemembership WHERE membership_id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    Ok(found.is_some())
}

/// Every scope of one type this principal may see — the list-query filter, served by the
/// `(principal_type, principal_id, scope_type)` index rather than by a second mechanism.
pub async fn scopes_for(
    pool: &PgPool,
    principal_type: UserType,
    principal_id: Uuid,
    scope_type: ScopeType,
) -> Result<Vec<Uuid>, DomainError> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT scope_id FROM scopemembership \
          WHERE principal_type = $1 AND principal_id = $2 AND scope_type = $3",
    )
    .bind(principal_type.to_text())
    .bind(principal_id)
    .bind(scope_type.to_text())
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// The [`ScopeMembershipRepository`] adapter — the one implementation both transports share
/// (GraphQL resolvers filter through it, a by-id fetch checks through it).
pub struct PgScopeMembershipRepository {
    pool: PgPool,
}

impl PgScopeMembershipRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ScopeMembershipRepository for PgScopeMembershipRepository {
    async fn is_member(
        &self,
        scope_type: ScopeType,
        scope_id: Uuid,
        scope: &ReadScope,
    ) -> Result<bool, DomainError> {
        match scope {
            // ADMIN holds no rows by design — storing them would be a row per admin per instance.
            // SYSTEM is a process manager/worker: unrestricted, but deliberately a distinct meaning.
            ReadScope::Admin | ReadScope::System => Ok(true),
            // An unauthenticated caller is never a member of anything. Public read models are reached
            // by not asking this question at all, not by answering it `true`.
            ReadScope::Public => Ok(false),
            _ => {
                let Some((principal_type, principal_id)) = scope.principal() else {
                    return Ok(false);
                };
                is_member(&self.pool, scope_type, scope_id, principal_type, principal_id).await
            }
        }
    }

    async fn scopes_for(
        &self,
        scope_type: ScopeType,
        scope: &ReadScope,
    ) -> Result<Vec<Uuid>, DomainError> {
        // ADMIN is deliberately NOT special-cased into "everything" here: an unbounded list is a
        // different question from an unbounded check, and callers that need it must ask the read
        // model directly rather than materialize every scope id in the system.
        let Some((principal_type, principal_id)) = scope.principal() else {
            return Ok(Vec::new());
        };
        scopes_for(&self.pool, principal_type, principal_id, scope_type).await
    }
}

/// How a [`ReadScope`] restricts a query — the ONE place the cases are decided, so no adapter
/// invents its own interpretation of `Admin` or `Public` (#144).
///
/// `Public` mapping to [`ScopePredicate::None`] rather than to "no restriction" is the whole point:
/// an unauthenticated caller reaching a tenant read model must get an EMPTY result, never everything.
/// That inversion is the classic authorization bug, so it is decided here once and not per adapter.
pub enum ScopePredicate {
    /// No restriction (ADMIN — it holds no membership rows by design — and SYSTEM machinery).
    All,
    /// Nothing is visible (PUBLIC on a tenant read model).
    None,
    /// Restricted to rows whose scope this principal is a member of:
    /// `(principal_type wire text, principal_id)`.
    Member(&'static str, Uuid),
}

pub fn scope_predicate(scope: &ReadScope) -> ScopePredicate {
    match scope {
        ReadScope::Admin | ReadScope::System => ScopePredicate::All,
        ReadScope::Public => ScopePredicate::None,
        _ => match scope.principal() {
            Some((principal_type, principal_id)) => {
                ScopePredicate::Member(principal_type.to_text(), principal_id)
            }
            None => ScopePredicate::None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::generated::scalars::{CustomerId, RestaurantAccountId, RestaurantId, RiderId};

    fn member_of(scope: &ReadScope) -> Option<(&'static str, Uuid)> {
        match scope_predicate(scope) {
            ScopePredicate::Member(t, id) => Some((t, id)),
            _ => None,
        }
    }

    /// THE authorization inversion, pinned. An unauthenticated caller reaching a tenant read model
    /// must see NOTHING. The tempting bug is to treat "no principal" as "no restriction" — which
    /// turns the most exposed caller into the most privileged one.
    #[test]
    fn public_sees_nothing_rather_than_everything() {
        assert!(matches!(scope_predicate(&ReadScope::Public), ScopePredicate::None));
    }

    /// ADMIN and SYSTEM are unrestricted — and hold no membership rows, which is why they must
    /// short-circuit rather than be looked up (a lookup would deny them).
    #[test]
    fn admin_and_system_are_unrestricted_without_a_lookup() {
        assert!(matches!(scope_predicate(&ReadScope::Admin), ScopePredicate::All));
        assert!(matches!(scope_predicate(&ReadScope::System), ScopePredicate::All));
    }

    /// Each tenant role resolves to its OWN principal_type wire value. A shared value would let one
    /// role's membership satisfy another's check — the reason principal_type is in the key at all.
    /// And the scope must carry the SAME id through — never rewrite the principal.
    #[test]
    fn every_tenant_role_maps_to_a_distinct_principal_type() {
        let id = Uuid::from_u128(7);
        let types: Vec<&str> = [
            ReadScope::Customer(CustomerId(id)),
            ReadScope::Restaurant(RestaurantId(id)),
            ReadScope::RestaurantAccount(RestaurantAccountId(id)),
            ReadScope::Rider(RiderId(id)),
        ]
        .iter()
        .map(|s| member_of(s).expect("tenant role is a member scope").0)
        .collect();

        let mut sorted = types.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), types.len(), "two roles share a principal_type value");
        for scope in [ReadScope::Customer(CustomerId(id)), ReadScope::Rider(RiderId(id))] {
            assert_eq!(member_of(&scope).unwrap().1, id);
        }
    }

    /// The stored values are the TEXT wire format of `UserType` (ADR-20260728), shared vocabulary
    /// with `domain_events.user_type`. Pinning them here makes a scalars.yaml variant rename fail
    /// loudly instead of silently stranding both this index and historical event rows.
    #[test]
    fn principal_type_wire_text_matches_the_usertype_declaration() {
        let id = Uuid::from_u128(1);
        assert_eq!(member_of(&ReadScope::Customer(CustomerId(id))).unwrap().0, "CUSTOMER");
        assert_eq!(
            member_of(&ReadScope::RestaurantAccount(RestaurantAccountId(id))).unwrap().0,
            "RESTAURANT_ACCOUNT"
        );
        assert_eq!(member_of(&ReadScope::Restaurant(RestaurantId(id))).unwrap().0, "RESTAURANT");
        assert_eq!(member_of(&ReadScope::Rider(RiderId(id))).unwrap().0, "RIDER");
        assert_eq!(ScopeType::ORDER.to_text(), "ORDER");
        assert_eq!(ScopeType::RESTAURANT.to_text(), "RESTAURANT");
    }
}
