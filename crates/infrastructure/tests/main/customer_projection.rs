//! Integration test for the Customer read-model slice: `verify_phone` with a verified-OTP fake auth
//! gateway (the register leg) → a `CustomerRegistered` row in `domain_events` → `ProjectionWorker`
//! folds it into the `customer` row → `PgCustomerRepository` serves it by phone / auth ref / id, and
//! the register-vs-identify resolution then finds the returning phone through the SAME Pg repository.
//! Needs a real Postgres: set `DATABASE_URL` (see restaurant_write_path.rs for a throwaway docker
//! one-liner). Without it the test SKIPS so `cargo test` stays green offline.

use application::commands::verify_phone;
use application::generated::services::{
    IdentitySendEmailMagicLinkInput, IdentitySendPhoneOtpInput, IdentityService,
    IdentityVerifyEmailTokenInput, IdentityVerifyEmailTokenOutput, IdentityVerifyPhoneOtpInput,
    IdentityVerifyPhoneOtpOutput, IdentityRefreshSessionInput, IdentityRefreshSessionOutput,
    IdentityStampCustomerClaimInput, IdentityStampMemberClaimInput, IdentityStampRiderClaimInput,
    ServiceCallMeta,
};
use application::ports::Actor;
use application::queries::CustomerReadRepository;
use async_trait::async_trait;
use domain::generated::commands::VerifyPhone;
use domain::generated::scalars::{
    AuthSubject, CustomerId, DialingCode, ExternalReference, Locale, NationalPhoneNumber, OtpCode,
    PhoneNumber,
    SessionId,
};
use domain::shared::errors::DomainError;
use infrastructure::{PgCustomerRepository, PgEventStore, ProjectionWorker};

/// Fake wrapped auth provider (Supabase ACL boundary, ADR-0015): every OTP verifies against a fixed
/// provider user reference — the register/identify leg under test starts AFTER the provider check.
struct AlwaysVerifiedAuth;

#[async_trait]
impl IdentityService for AlwaysVerifiedAuth {
    async fn send_phone_otp(
        &self,
        _input: IdentitySendPhoneOtpInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        Ok(())
    }

    async fn verify_phone_otp(
        &self,
        _input: IdentityVerifyPhoneOtpInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityVerifyPhoneOtpOutput, DomainError> {
        Ok(IdentityVerifyPhoneOtpOutput {
            auth_ref: AuthSubject("auth-supabase-1".into()),
            access_token: None,
            refresh_token: None,
            expires_in: None,
        })
    }

    async fn refresh_session(
        &self,
        _input: IdentityRefreshSessionInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityRefreshSessionOutput, DomainError> {
        Ok(IdentityRefreshSessionOutput { access_token: "t".into(), refresh_token: None, expires_in: None })
    }

    async fn stamp_customer_claim(
        &self,
        _input: IdentityStampCustomerClaimInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        Ok(())
    }

    async fn stamp_rider_claim(
        &self,
        _input: IdentityStampRiderClaimInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        Ok(())
    }

    async fn send_email_magic_link(
        &self,
        _input: IdentitySendEmailMagicLinkInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        Ok(())
    }

    async fn send_admin_sign_in_link(
        &self,
        _input: application::generated::services::IdentitySendAdminSignInLinkInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        Ok(())
    }

    async fn verify_email_token(
        &self,
        _input: IdentityVerifyEmailTokenInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityVerifyEmailTokenOutput, DomainError> {
        Err(DomainError::rejected("InvalidVerificationToken", serde_json::json!({})))
    }

    async fn stamp_member_claim(
        &self,
        _input: IdentityStampMemberClaimInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        Ok(())
    }

    async fn stamp_admin_claim(
        &self,
        _input: application::generated::services::IdentityStampAdminClaimInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        Ok(())
    }
}

fn verify_phone_cmd(customer_id: uuid::Uuid) -> VerifyPhone {
    VerifyPhone {
        customer_id: CustomerId(customer_id),
        dialing_code: DialingCode("+33".into()),
        national_number: NationalPhoneNumber("0612345678".into()),
        code: OtpCode("123456".into()),
        session_id: SessionId(uuid::Uuid::new_v4()),
        display_name: None,
        locale: Some(Locale("fr-FR".into())),
        timezone: None,
    }
}

#[tokio::test]
async fn registered_customer_is_folded_and_served_by_the_read_repository() {
    let Some(db) = crate::common::TestDb::acquire("customer_projection").await else { return };
    let pool = db.pool();

    let store = PgEventStore::new(pool.clone());
    let repo = PgCustomerRepository::new(pool.clone());
    let actor = Actor {
        user_id: uuid::Uuid::nil(),
        user_type: "CUSTOMER".to_string(),
        domain_id: None,
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    };

    // 1) First verified phone → the register leg: CustomerRegistered on the new Customer-<id> stream.
    let customer_id = uuid::Uuid::new_v4();
    let outcome = verify_phone(&store, &AlwaysVerifiedAuth, &repo, &application::auth_sessions::mem::MemAuthSessionStore::default(), verify_phone_cmd(customer_id), &actor)
        .await
        .expect("verify_phone (register)");
    assert!(outcome.created, "unknown phone registers a new customer");
    assert_eq!(outcome.customer_id, CustomerId(customer_id));
    let (stream, event_type): (String, String) =
        sqlx::query_as("SELECT stream_name, event_type FROM domain_events ORDER BY position DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("registered event row");
    assert_eq!(stream, format!("Customer-{customer_id}"));
    assert_eq!(event_type, "CustomerRegistered");

    // 2) One worker pass folds it into the `customer` row: canonical E.164 phone, linked authRef,
    //    empty accumulations.
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once");
    let row = repo
        .by_phone(PhoneNumber("+33612345678".into()))
        .await
        .expect("by_phone")
        .expect("projected customer row");
    assert_eq!(row.customer_id, CustomerId(customer_id));
    assert_eq!(row.auth_ref, Some(AuthSubject("auth-supabase-1".into())));
    assert!(!row.email_verified);
    assert_eq!(row.favorite_restaurant_ids, serde_json::json!([]));

    // 3) The other lookups serve the same row: by auth ref (the `me` query) and by id.
    let by_ref = repo
        // Both scalars in one call, deliberately visible: the ROW holds an `AuthSubject`, while the
        // read PORT still takes an `ExternalReference` until step 1b retypes it (#639 part C).
        .by_auth_ref(ExternalReference("auth-supabase-1".into()))
        .await
        .expect("by_auth_ref")
        .expect("customer by auth ref");
    assert_eq!(by_ref.customer_id, CustomerId(customer_id));
    let by_id = repo.by_id(CustomerId(customer_id)).await.expect("by_id").expect("customer by id");
    assert_eq!(by_id.phone, PhoneNumber("+33612345678".into()));

    // 4) The RETURNING phone resolves through the SAME Pg repository → the identify leg: the
    //    client-proposed id is discarded and CustomerIdentified lands on the EXISTING stream.
    let proposed = uuid::Uuid::new_v4();
    let outcome = verify_phone(&store, &AlwaysVerifiedAuth, &repo, &application::auth_sessions::mem::MemAuthSessionStore::default(), verify_phone_cmd(proposed), &actor)
        .await
        .expect("verify_phone (identify)");
    assert!(!outcome.created, "known phone identifies, never re-registers");
    assert_eq!(outcome.customer_id, CustomerId(customer_id));
    let (stream, event_type): (String, String) =
        sqlx::query_as("SELECT stream_name, event_type FROM domain_events ORDER BY position DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("identified event row");
    assert_eq!(stream, format!("Customer-{customer_id}"));
    assert_eq!(event_type, "CustomerIdentified");
}
