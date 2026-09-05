//! `RestaurantInvitationList` read model (#639 part C step 6-iv round 2, ADR-20260905-101349 §2
//! amendment, PROP-20260831-180622 §6.4): the executable rebuild recipe -- TRUNCATE + reset
//! TOGETHER (the table's own `rules:`, opposite of `RestaurantRoster`'s) -- plus the status
//! transitions the invite/accept/revoke/expire legs produce. No predicate here is ever a
//! revocation test in the `NOT EXISTS` sense (PROP §6.4): every assertion is a positive
//! existence + status-value check.

use application::commands::{invite_restaurant_member, revoke_restaurant_invitation};
use application::ports::Actor;
use domain::generated::commands::{InviteRestaurantMember, RevokeRestaurantInvitation};
use domain::generated::scalars::{EmailAddress, MemberAuthority, MemberId, RestaurantId, RestaurantInvitationId};
use infrastructure::{PgEventStore, ProjectionWorker};
use sqlx::{PgPool, Row};

fn actor() -> Actor {
    Actor {
        user_id: uuid::Uuid::nil(),
        user_type: "RESTAURANT".to_string(),
        domain_id: None,
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    }
}

async fn invite(
    pool: &PgPool,
    invitation_id: uuid::Uuid,
    restaurant_id: uuid::Uuid,
    member_id: uuid::Uuid,
    email: &str,
    authority: MemberAuthority,
) -> Result<(), domain::shared::errors::DomainError> {
    let store = PgEventStore::new(pool.clone());
    invite_restaurant_member(
        &store,
        InviteRestaurantMember {
            invitation_id: RestaurantInvitationId(invitation_id),
            restaurant_id: RestaurantId(restaurant_id),
            invited_email: EmailAddress(email.to_string()),
            authority,
            member_id: MemberId(member_id),
        },
        &actor(),
        true,
    )
    .await
}

async fn revoke(pool: &PgPool, invitation_id: uuid::Uuid) -> Result<(), domain::shared::errors::DomainError> {
    let store = PgEventStore::new(pool.clone());
    revoke_restaurant_invitation(
        &store,
        RevokeRestaurantInvitation { invitation_id: RestaurantInvitationId(invitation_id) },
        &actor(),
    )
    .await
}

async fn status_of(pool: &PgPool, invitation_id: uuid::Uuid) -> Option<String> {
    sqlx::query("SELECT status FROM restaurantinvitationlist WHERE invitation_id = $1")
        .bind(invitation_id)
        .fetch_optional(pool)
        .await
        .expect("query restaurantinvitationlist")
        .map(|r| r.get::<String, _>("status"))
}

/// A GRANT-shaped predicate: PENDING lands with the row, then REVOKED overwrites it in place.
#[tokio::test]
async fn invite_projects_pending_then_revoke_overwrites_it() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_invitation_list_pending_revoked").await else {
        return;
    };
    let pool = db.pool();
    let invitation = uuid::Uuid::new_v4();
    invite(&pool, invitation, uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), "colleague@example.com", MemberAuthority::OPERATOR)
        .await
        .expect("invite");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (invite)");
    assert_eq!(status_of(&pool, invitation).await.as_deref(), Some("PENDING"));

    revoke(&pool, invitation).await.expect("revoke");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (revoke)");
    assert_eq!(status_of(&pool, invitation).await.as_deref(), Some("REVOKED"));
}

/// `expiresAt` is computed (occurred_at + RESTAURANT_INVITATION_TTL_SECONDS), never null once born.
#[tokio::test]
async fn expires_at_is_computed_from_the_sent_facts_occurred_at() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_invitation_list_expires_at").await else {
        return;
    };
    let pool = db.pool();
    let invitation = uuid::Uuid::new_v4();
    invite(&pool, invitation, uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), "ttl@example.com", MemberAuthority::MANAGER)
        .await
        .expect("invite");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once");

    let row = sqlx::query("SELECT created_at, expires_at FROM restaurantinvitationlist WHERE invitation_id = $1")
        .bind(invitation)
        .fetch_one(&pool)
        .await
        .expect("row exists");
    let created_at: chrono::DateTime<chrono::Utc> = row.get("created_at");
    let expires_at: Option<chrono::DateTime<chrono::Utc>> = row.get("expires_at");
    let expires_at = expires_at.expect("expiresAt is never null once born");
    let delta = (expires_at - created_at).num_seconds();
    assert_eq!(delta, 604_800, "the SPEC DEFAULT TTL (604800s / 7 days) — see the projector's own doc comment for why this reads the spec default, not a live env override");
}

/// The executable rebuild recipe (dba, PROP §6.4): TRUNCATE + reset TOGETHER. Right after the
/// TRUNCATE, before the replay runs, the row is GONE -- a real, named window (the `Member`
/// precedent's `truncate_then_reset_opens_a_real_denial_window`, transposed) -- then the replay
/// heals it back to the SAME status.
#[tokio::test]
async fn truncate_and_reset_together_opens_a_window_then_the_replay_heals_it() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_invitation_list_truncate_reset").await else {
        return;
    };
    let pool = db.pool();
    let invitation = uuid::Uuid::new_v4();
    invite(&pool, invitation, uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), "rebuild@example.com", MemberAuthority::MANAGER)
        .await
        .expect("invite");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (first drain)");
    assert_eq!(status_of(&pool, invitation).await.as_deref(), Some("PENDING"));

    sqlx::query("TRUNCATE restaurantinvitationlist").execute(&pool).await.expect("truncate");
    sqlx::query("DELETE FROM projection_checkpoint WHERE projector = 'RestaurantInvitationList'")
        .execute(&pool)
        .await
        .expect("reset the RestaurantInvitationList checkpoint to 0");

    assert_eq!(
        status_of(&pool, invitation).await,
        None,
        "TRUNCATE opens a real window in which an invitation that exists resolves to nothing -- \
         exactly why the table's own `rules:` require reset TOGETHER, never checkpoint-only here"
    );

    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (replay heals it)");
    assert_eq!(status_of(&pool, invitation).await.as_deref(), Some("PENDING"), "the replay reproduces the same status");
}
