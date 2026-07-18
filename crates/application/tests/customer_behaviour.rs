//! BEHAVIOUR tests for the Customer aggregate — the executable form of the `specs/tests.yaml`
//! Given/When/Then cases whose `when` is a Customer command (ADR-0032: each test cites the
//! `specs/rules.yaml` rule it asserts). Given = pre-seeded stream events (in-memory event store),
//! When = the command handler, Then = the emitted event(s) / the errors.yaml rejection code.
//!
//! Pure and offline: the identity flows are WRAPPED Supabase Auth (ADR-0015), so the tests fake the
//! `AuthProviderGateway` (the ACL boundary — OTP "123456" is valid, "999999" expired, anything else
//! invalid; magic-link token "sb-magic-token-abc" proves johnny@example.com) and the
//! `CustomerReadRepository` (the phone/email uniqueness-and-resolution index).

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use application::commands::{
    change_language, confirm_email_verification, confirm_phone_change, mark_restaurant_as_favorite,
    rejection_code, remove_customer_address, request_email_verification, request_phone_change,
    request_phone_verification, set_customer_address, set_customer_payment_method,
    set_customer_preferences, unmark_restaurant_as_favorite, update_customer_info, verify_phone,
};
use application::ports::{
    version_conflict, Actor, AuthProviderGateway, EmailTokenCheck, EventStore, PhoneOtpCheck,
};
use application::queries::{
    CustomerReadRepository, CustomerRow, RestaurantFilter, RestaurantReadRepository, RestaurantRow,
};
use domain::generated::commands::{
    ChangeLanguage, ConfirmEmailVerification, ConfirmPhoneChange, MarkRestaurantAsFavorite,
    RemoveCustomerAddress, RequestEmailVerification, RequestPhoneChange, RequestPhoneVerification,
    SetCustomerAddress, SetCustomerPaymentMethod, SetCustomerPreferences, UnmarkRestaurantAsFavorite,
    UpdateCustomerInfo, VerifyPhone,
};
use domain::generated::entities::Address;
use domain::generated::events::{CustomerRegistered, DomainEvent, RestaurantFavorited};
use domain::generated::scalars::*;
use domain::shared::errors::DomainError;

// ------------------------------------------------------------------------------------------------
// Test doubles
// ------------------------------------------------------------------------------------------------

/// In-memory [`EventStore`]: version = number of events on the stream, same optimistic-concurrency
/// semantics as `PgEventStore` (a clash → the canonical `version_conflict`).
#[derive(Default)]
struct MemStore {
    streams: Mutex<HashMap<String, Vec<DomainEvent>>>,
}

impl MemStore {
    /// GIVEN: pre-seed a stream with already-recorded facts.
    fn seed(&self, stream: &str, events: Vec<DomainEvent>) {
        self.streams.lock().unwrap().insert(stream.to_string(), events);
    }

    /// THEN: the full stream after the command ran.
    fn stream(&self, stream: &str) -> Vec<DomainEvent> {
        self.streams.lock().unwrap().get(stream).cloned().unwrap_or_default()
    }
}

#[async_trait]
impl EventStore for MemStore {
    async fn append(
        &self,
        stream_name: &str,
        expected_version: i64,
        events: &[DomainEvent],
        _actor: &Actor,
    ) -> Result<i64, DomainError> {
        let mut streams = self.streams.lock().unwrap();
        let stream = streams.entry(stream_name.to_string()).or_default();
        if stream.len() as i64 != expected_version {
            return Err(version_conflict(stream_name, expected_version));
        }
        stream.extend(events.iter().cloned());
        Ok(stream.len() as i64)
    }

    async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        let events = self.stream(stream_name);
        let version = events.len() as i64;
        Ok((events, version))
    }
}

/// Fake wrapped auth provider (the Supabase ACL boundary): fixed OTP/token semantics and a log of
/// the send effects, so `request*` tests can assert the delegation happened.
#[derive(Default)]
struct FakeAuth {
    sends: Mutex<Vec<String>>,
}

impl FakeAuth {
    fn sends(&self) -> Vec<String> {
        self.sends.lock().unwrap().clone()
    }
}

#[async_trait]
impl AuthProviderGateway for FakeAuth {
    async fn send_phone_otp(
        &self,
        dialing_code: &DialingCode,
        national_number: &NationalPhoneNumber,
        locale: Option<&Locale>,
    ) -> Result<(), DomainError> {
        self.sends.lock().unwrap().push(format!(
            "otp:{}{}:{}",
            dialing_code.0,
            national_number.0,
            locale.map(|l| l.0.as_str()).unwrap_or("-")
        ));
        Ok(())
    }

    async fn verify_phone_otp(
        &self,
        _dialing_code: &DialingCode,
        _national_number: &NationalPhoneNumber,
        code: &OtpCode,
    ) -> Result<PhoneOtpCheck, DomainError> {
        Ok(match code.0.as_str() {
            "123456" => PhoneOtpCheck::Verified { auth_ref: ExternalReference("auth-supabase-1".into()) },
            "999999" => PhoneOtpCheck::Expired,
            _ => PhoneOtpCheck::Invalid,
        })
    }

    async fn send_email_magic_link(
        &self,
        email: &EmailAddress,
        locale: Option<&Locale>,
    ) -> Result<(), DomainError> {
        self.sends.lock().unwrap().push(format!(
            "magic-link:{}:{}",
            email.0,
            locale.map(|l| l.0.as_str()).unwrap_or("-")
        ));
        Ok(())
    }

    async fn verify_email_token(
        &self,
        token: &EmailVerificationToken,
    ) -> Result<EmailTokenCheck, DomainError> {
        Ok(match token.0.as_str() {
            "sb-magic-token-abc" => {
                EmailTokenCheck::Verified { email: EmailAddress("johnny@example.com".into()) }
            }
            "expired-token" => EmailTokenCheck::Expired,
            _ => EmailTokenCheck::Invalid,
        })
    }
}

/// Fake Customer identity/lookup read model (phone/email index).
#[derive(Default)]
struct FakeCustomers {
    rows: Vec<CustomerRow>,
}

#[async_trait]
impl CustomerReadRepository for FakeCustomers {
    async fn by_phone(&self, phone: PhoneNumber) -> Result<Option<CustomerRow>, DomainError> {
        Ok(self.rows.iter().find(|r| r.phone == phone).cloned())
    }

    async fn by_email(&self, email: EmailAddress) -> Result<Option<CustomerRow>, DomainError> {
        Ok(self.rows.iter().find(|r| r.email.as_ref() == Some(&email)).cloned())
    }
}

/// Fake Restaurant read model (favorite target existence).
#[derive(Default)]
struct FakeRestaurants {
    row: Option<RestaurantRow>,
}

#[async_trait]
impl RestaurantReadRepository for FakeRestaurants {
    async fn list(&self, _filter: RestaurantFilter) -> Result<Vec<RestaurantRow>, DomainError> {
        Ok(self.row.clone().into_iter().collect())
    }

    async fn by_slug(&self, slug: Slug) -> Result<Option<RestaurantRow>, DomainError> {
        Ok(self.row.clone().filter(|r| r.slug == slug))
    }

    async fn by_id(&self, id: RestaurantId) -> Result<Option<RestaurantRow>, DomainError> {
        Ok(self.row.clone().filter(|r| r.restaurant_id == id))
    }
}

// ------------------------------------------------------------------------------------------------
// Fixtures (tests.yaml `fixtures`, with UUIDs instead of the sample string ids)
// ------------------------------------------------------------------------------------------------

fn actor() -> Actor {
    Actor {
        user_id: uuid::Uuid::new_v4(),
        user_type: 1, // UserType::CUSTOMER ordinal
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    }
}

fn stream(id: CustomerId) -> String {
    format!("Customer-{}", id.0)
}

/// Fixture `customerRegistered` (phone +33612345678, "Johnny", fr-FR).
fn registered_event(id: CustomerId) -> DomainEvent {
    DomainEvent::CustomerRegistered(CustomerRegistered {
        mode: None,
        customer_id: id,
        auth_ref: None,
        phone: PhoneNumber("+33612345678".into()),
        display_name: Some(CustomerDisplayName("Johnny".into())),
        email: None,
        locale: Some(Locale("fr-FR".into())),
        timezone: Some(TimeZone("Europe/Paris".into())),
    })
}

/// The projected `customer` row matching [`registered_event`].
fn projected_row(id: CustomerId, phone: &str, email: Option<&str>) -> CustomerRow {
    CustomerRow {
        customer_id: id,
        phone: PhoneNumber(phone.into()),
        auth_ref: None,
        display_name: Some(CustomerDisplayName("Johnny".into())),
        email: email.map(|e| EmailAddress(e.into())),
        email_verified: email.is_some(),
        locale: Some(Locale("fr-FR".into())),
        timezone: Some(TimeZone("Europe/Paris".into())),
        ratings: serde_json::json!([]),
        favorite_restaurant_ids: serde_json::json!([]),
        preferences: None,
        addresses: serde_json::json!([]),
        payment_method_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn address() -> Address {
    Address {
        line1: AddressLine("1 Rue Nationale".into()),
        line2: None,
        postal_code: PostalCode("37000".into()),
        city: CityName("Tours".into()),
        country: CountryCode("FR".into()),
    }
}

fn verify_phone_cmd(id: CustomerId, code: &str) -> VerifyPhone {
    VerifyPhone {
        customer_id: id,
        dialing_code: DialingCode("+33".into()),
        national_number: NationalPhoneNumber("612345678".into()),
        code: OtpCode(code.into()),
        display_name: Some(CustomerDisplayName("Johnny".into())),
        locale: Some(Locale("fr-FR".into())),
        timezone: Some(TimeZone("Europe/Paris".into())),
    }
}

fn cust() -> CustomerId {
    CustomerId(uuid::Uuid::new_v4())
}

fn rid() -> RestaurantId {
    RestaurantId(uuid::Uuid::new_v4())
}

// ------------------------------------------------------------------------------------------------
// Phone verification: register-or-identify (rules.yaml#/PhoneVerificationRegistersOrIdentifies)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCustomerRequestPhoneVerification —
/// rules.yaml#/PhoneVerificationRegistersOrIdentifies (pure effect: OTP delegated, nothing emitted)
#[tokio::test]
async fn requesting_phone_verification_delegates_the_otp_and_emits_nothing() {
    let store = MemStore::default();
    let auth = FakeAuth::default();

    request_phone_verification(
        &store,
        &auth,
        RequestPhoneVerification {
            dialing_code: DialingCode("+33".into()),
            national_number: NationalPhoneNumber("612345678".into()),
            locale: Some(Locale("fr-FR".into())),
        },
        &actor(),
    )
    .await
    .expect("request");

    assert_eq!(auth.sends(), vec!["otp:+33612345678:fr-FR".to_string()]);
    assert!(store.streams.lock().unwrap().is_empty(), "no event");
}

/// tests.yaml#/cases/TestCustomerVerifyPhoneRegisters —
/// rules.yaml#/PhoneVerificationRegistersOrIdentifies
#[tokio::test]
async fn verifying_the_otp_on_a_new_phone_registers_the_customer() {
    let store = MemStore::default();
    let id = cust();

    let outcome = verify_phone(
        &store,
        &FakeAuth::default(),
        &FakeCustomers::default(),
        verify_phone_cmd(id, "123456"),
        &actor(),
    )
    .await
    .expect("verify");

    assert_eq!(outcome.customer_id, id);
    assert!(outcome.created);
    let events = store.stream(&stream(id));
    assert_eq!(events.len(), 1);
    let DomainEvent::CustomerRegistered(e) = &events[0] else {
        panic!("expected CustomerRegistered, got {:?}", events[0]);
    };
    assert_eq!(e.phone.0, "+33612345678", "canonical E.164 from dialing code + national number");
    assert_eq!(e.auth_ref.as_ref().map(|r| r.0.as_str()), Some("auth-supabase-1"));
    assert!(e.email.is_none(), "email is verified-only, never set at registration");
}

/// tests.yaml#/cases/TestCustomerVerifyPhoneReturningIdentifies —
/// rules.yaml#/PhoneVerificationRegistersOrIdentifies
#[tokio::test]
async fn verifying_the_otp_on_a_known_phone_identifies_the_returning_customer() {
    let store = MemStore::default();
    let existing = cust();
    store.seed(&stream(existing), vec![registered_event(existing)]);
    let customers = FakeCustomers { rows: vec![projected_row(existing, "+33612345678", None)] };

    // The client proposes a FRESH id; the backend resolves the phone to the existing customer.
    let proposed = cust();
    let outcome = verify_phone(
        &store,
        &FakeAuth::default(),
        &customers,
        verify_phone_cmd(proposed, "123456"),
        &actor(),
    )
    .await
    .expect("identify");

    assert_eq!(outcome.customer_id, existing, "the proposed id is discarded");
    assert!(!outcome.created);
    let events = store.stream(&stream(existing));
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[1],
        DomainEvent::CustomerIdentified(e)
            if e.customer_id == existing && e.auth_ref.0 == "auth-supabase-1"
    ));
    assert!(store.stream(&stream(proposed)).is_empty(), "no stream for the proposed id");
}

/// tests.yaml#/cases/TestCustomerVerifyPhoneIsRejected (both arms) —
/// rules.yaml#/PhoneVerificationRegistersOrIdentifies
#[tokio::test]
async fn rejects_phone_verification_when_the_otp_is_wrong_or_expired() {
    let store = MemStore::default();
    let id = cust();

    let err = verify_phone(
        &store,
        &FakeAuth::default(),
        &FakeCustomers::default(),
        verify_phone_cmd(id, "000000"),
        &actor(),
    )
    .await
    .expect_err("wrong code");
    assert_eq!(rejection_code(&err), Some("InvalidVerificationCode"));

    let err = verify_phone(
        &store,
        &FakeAuth::default(),
        &FakeCustomers::default(),
        verify_phone_cmd(id, "999999"),
        &actor(),
    )
    .await
    .expect_err("expired code");
    assert_eq!(rejection_code(&err), Some("VerificationCodeExpired"));
    assert!(store.stream(&stream(id)).is_empty(), "no event on rejection");
}

// ------------------------------------------------------------------------------------------------
// Email verification (rules.yaml#/EmailVerificationUniqueTokenValid)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCustomerRequestEmailVerification —
/// rules.yaml#/EmailVerificationUniqueTokenValid (pure effect, localized via the STORED locale)
#[tokio::test]
async fn requesting_email_verification_sends_the_magic_link_and_emits_nothing() {
    let store = MemStore::default();
    let auth = FakeAuth::default();
    let id = cust();
    store.seed(&stream(id), vec![registered_event(id)]);

    request_email_verification(
        &store,
        &auth,
        &FakeCustomers::default(),
        RequestEmailVerification { customer_id: id, email: EmailAddress("johnny@example.com".into()) },
        &actor(),
    )
    .await
    .expect("request");

    // Localized by the STORED fr-FR locale (ADR-0015: no per-call language param).
    assert_eq!(auth.sends(), vec!["magic-link:johnny@example.com:fr-FR".to_string()]);
    assert_eq!(store.stream(&stream(id)).len(), 1, "no event");
}

/// tests.yaml#/cases/TestCustomerRequestEmailVerificationDuplicateIsRejected —
/// rules.yaml#/EmailVerificationUniqueTokenValid
#[tokio::test]
async fn rejects_requesting_email_verification_for_an_email_owned_by_another_account() {
    let store = MemStore::default();
    let id = cust();
    store.seed(&stream(id), vec![registered_event(id)]);
    let customers = FakeCustomers {
        rows: vec![projected_row(cust(), "+33700000000", Some("taken@example.com"))],
    };

    let err = request_email_verification(
        &store,
        &FakeAuth::default(),
        &customers,
        RequestEmailVerification { customer_id: id, email: EmailAddress("taken@example.com".into()) },
        &actor(),
    )
    .await
    .expect_err("email taken");
    assert_eq!(rejection_code(&err), Some("EmailAlreadyInUse"));
}

/// tests.yaml#/cases/TestCustomerConfirmEmailVerification —
/// rules.yaml#/EmailVerificationUniqueTokenValid
#[tokio::test]
async fn confirming_the_magic_link_links_the_verified_email() {
    let store = MemStore::default();
    let id = cust();
    store.seed(&stream(id), vec![registered_event(id)]);

    confirm_email_verification(
        &store,
        &FakeAuth::default(),
        ConfirmEmailVerification {
            customer_id: id,
            token: EmailVerificationToken("sb-magic-token-abc".into()),
        },
        &actor(),
    )
    .await
    .expect("confirm");

    let events = store.stream(&stream(id));
    assert!(matches!(
        &events[1],
        DomainEvent::CustomerEmailVerified(e) if e.email.0 == "johnny@example.com"
    ));
}

/// tests.yaml#/cases/TestCustomerConfirmEmailInvalidTokenIsRejected (both arms) —
/// rules.yaml#/EmailVerificationUniqueTokenValid
#[tokio::test]
async fn rejects_email_confirmation_on_an_invalid_or_expired_token() {
    let store = MemStore::default();
    let id = cust();
    store.seed(&stream(id), vec![registered_event(id)]);

    let err = confirm_email_verification(
        &store,
        &FakeAuth::default(),
        ConfirmEmailVerification { customer_id: id, token: EmailVerificationToken("bad-token".into()) },
        &actor(),
    )
    .await
    .expect_err("bad token");
    assert_eq!(rejection_code(&err), Some("InvalidVerificationToken"));

    let err = confirm_email_verification(
        &store,
        &FakeAuth::default(),
        ConfirmEmailVerification {
            customer_id: id,
            token: EmailVerificationToken("expired-token".into()),
        },
        &actor(),
    )
    .await
    .expect_err("expired token");
    assert_eq!(rejection_code(&err), Some("VerificationCodeExpired"));
    assert_eq!(store.stream(&stream(id)).len(), 1, "no event on rejection");
}

// ------------------------------------------------------------------------------------------------
// Phone change (rules.yaml#/PhoneChangeVerifiedAndUnique)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCustomerRequestPhoneChange — rules.yaml#/PhoneChangeVerifiedAndUnique
#[tokio::test]
async fn requesting_a_phone_change_sends_the_otp_to_the_new_phone_and_emits_nothing() {
    let store = MemStore::default();
    let auth = FakeAuth::default();
    let id = cust();
    store.seed(&stream(id), vec![registered_event(id)]);

    request_phone_change(
        &store,
        &auth,
        &FakeCustomers::default(),
        RequestPhoneChange {
            customer_id: id,
            new_dialing_code: DialingCode("+33".into()),
            new_national_number: NationalPhoneNumber("699999999".into()),
        },
        &actor(),
    )
    .await
    .expect("request");

    assert_eq!(auth.sends(), vec!["otp:+33699999999:fr-FR".to_string()]);
    assert_eq!(store.stream(&stream(id)).len(), 1, "no event");
}

/// tests.yaml#/cases/TestCustomerRequestPhoneChangeDuplicateIsRejected —
/// rules.yaml#/PhoneChangeVerifiedAndUnique
#[tokio::test]
async fn rejects_changing_to_a_phone_owned_by_another_account() {
    let store = MemStore::default();
    let id = cust();
    store.seed(&stream(id), vec![registered_event(id)]);
    let customers =
        FakeCustomers { rows: vec![projected_row(cust(), "+33600000000", None)] };

    let err = request_phone_change(
        &store,
        &FakeAuth::default(),
        &customers,
        RequestPhoneChange {
            customer_id: id,
            new_dialing_code: DialingCode("+33".into()),
            new_national_number: NationalPhoneNumber("600000000".into()),
        },
        &actor(),
    )
    .await
    .expect_err("phone taken");
    assert_eq!(rejection_code(&err), Some("PhoneAlreadyInUse"));
}

/// tests.yaml#/cases/TestCustomerConfirmPhoneChange — rules.yaml#/PhoneChangeVerifiedAndUnique
#[tokio::test]
async fn confirming_the_otp_on_the_new_phone_changes_the_phone() {
    let store = MemStore::default();
    let id = cust();
    store.seed(&stream(id), vec![registered_event(id)]);

    confirm_phone_change(
        &store,
        &FakeAuth::default(),
        &FakeCustomers::default(),
        ConfirmPhoneChange {
            customer_id: id,
            new_dialing_code: DialingCode("+33".into()),
            new_national_number: NationalPhoneNumber("699999999".into()),
            code: OtpCode("123456".into()),
        },
        &actor(),
    )
    .await
    .expect("confirm");

    let events = store.stream(&stream(id));
    assert!(matches!(
        &events[1],
        DomainEvent::CustomerPhoneChanged(e) if e.phone.0 == "+33699999999"
    ));

    // A wrong OTP rejects (actors.yaml throws InvalidVerificationCode).
    let err = confirm_phone_change(
        &store,
        &FakeAuth::default(),
        &FakeCustomers::default(),
        ConfirmPhoneChange {
            customer_id: id,
            new_dialing_code: DialingCode("+33".into()),
            new_national_number: NationalPhoneNumber("699999999".into()),
            code: OtpCode("000000".into()),
        },
        &actor(),
    )
    .await
    .expect_err("wrong code");
    assert_eq!(rejection_code(&err), Some("InvalidVerificationCode"));
}

// ------------------------------------------------------------------------------------------------
// Profile & preferences (rules.yaml#/CustomerProfileUpdate, #/CustomerPreferences)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCustomerChangeLanguage — rules.yaml#/CustomerProfileUpdate
#[tokio::test]
async fn persists_the_preferred_language() {
    let store = MemStore::default();
    let id = cust();
    store.seed(&stream(id), vec![registered_event(id)]);

    change_language(
        &store,
        ChangeLanguage { customer_id: id, locale: Locale("en-US".into()) },
        &actor(),
    )
    .await
    .expect("change language");

    let events = store.stream(&stream(id));
    assert!(matches!(
        &events[1],
        DomainEvent::CustomerLanguageChanged(e) if e.locale.0 == "en-US"
    ));
}

/// tests.yaml#/cases/TestCustomerInfoUpdated + TestCustomerUpdateWithoutFieldIsRejected —
/// rules.yaml#/CustomerProfileUpdate
#[tokio::test]
async fn updates_the_display_name_and_rejects_an_empty_update() {
    let store = MemStore::default();
    let id = cust();
    store.seed(&stream(id), vec![registered_event(id)]);

    update_customer_info(
        &store,
        UpdateCustomerInfo {
            customer_id: id,
            display_name: Some(CustomerDisplayName("Johnny B.".into())),
        },
        &actor(),
    )
    .await
    .expect("update info");
    let events = store.stream(&stream(id));
    assert!(matches!(
        &events[1],
        DomainEvent::CustomerInfoUpdated(e)
            if e.display_name.as_ref().map(|n| n.0.as_str()) == Some("Johnny B.")
    ));

    let err = update_customer_info(
        &store,
        UpdateCustomerInfo { customer_id: id, display_name: None },
        &actor(),
    )
    .await
    .expect_err("empty update");
    assert_eq!(rejection_code(&err), Some("NoEditableFieldProvided"));
    assert_eq!(store.stream(&stream(id)).len(), 2, "no event on rejection");
}

/// tests.yaml#/cases/TestCustomerPreferencesSet — rules.yaml#/CustomerPreferences
#[tokio::test]
async fn sets_discovery_preferences() {
    let store = MemStore::default();
    let id = cust();
    store.seed(&stream(id), vec![registered_event(id)]);

    set_customer_preferences(
        &store,
        SetCustomerPreferences {
            customer_id: id,
            timezone: Some(TimeZone("Europe/Paris".into())),
            dietary_tags: vec![Tag("vegan".into())],
            favorite_cuisines: vec![],
        },
        &actor(),
    )
    .await
    .expect("set preferences");

    let events = store.stream(&stream(id));
    assert!(matches!(
        &events[1],
        DomainEvent::CustomerPreferencesSet(e)
            if e.dietary_tags.len() == 1 && e.dietary_tags[0].0 == "vegan"
    ));
}

// ------------------------------------------------------------------------------------------------
// Favorites (rules.yaml#/FavoritesManagement)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCustomerRestaurantFavorited + TestCustomerFavoriteUnknownRestaurantIsRejected
/// — rules.yaml#/FavoritesManagement
#[tokio::test]
async fn favorites_an_existing_restaurant_and_rejects_an_unknown_one() {
    let store = MemStore::default();
    let id = cust();
    store.seed(&stream(id), vec![registered_event(id)]);
    let restaurant_id = rid();
    let restaurants = FakeRestaurants {
        row: Some(minimal_restaurant_row(restaurant_id)),
    };

    mark_restaurant_as_favorite(
        &store,
        &restaurants,
        MarkRestaurantAsFavorite { customer_id: id, restaurant_id },
        &actor(),
    )
    .await
    .expect("favorite");
    let events = store.stream(&stream(id));
    assert!(matches!(
        &events[1],
        DomainEvent::RestaurantFavorited(e) if e.restaurant_id == restaurant_id
    ));

    let err = mark_restaurant_as_favorite(
        &store,
        &restaurants,
        MarkRestaurantAsFavorite { customer_id: id, restaurant_id: rid() },
        &actor(),
    )
    .await
    .expect_err("unknown restaurant");
    assert_eq!(rejection_code(&err), Some("RestaurantNotFound"));
}

/// tests.yaml#/cases/TestCustomerRestaurantUnfavorited + TestCustomerUnfavoriteNonFavoriteIsNoOp —
/// rules.yaml#/FavoritesManagement
#[tokio::test]
async fn unfavorites_a_favorite_and_ignores_a_non_favorite() {
    let store = MemStore::default();
    let id = cust();
    let restaurant_id = rid();
    store.seed(
        &stream(id),
        vec![
            registered_event(id),
            DomainEvent::RestaurantFavorited(RestaurantFavorited {
                customer_id: id,
                restaurant_id,
            }),
        ],
    );

    unmark_restaurant_as_favorite(
        &store,
        UnmarkRestaurantAsFavorite { customer_id: id, restaurant_id },
        &actor(),
    )
    .await
    .expect("unfavorite");
    let events = store.stream(&stream(id));
    assert_eq!(events.len(), 3);
    assert!(matches!(
        &events[2],
        DomainEvent::RestaurantUnfavorited(e) if e.restaurant_id == restaurant_id
    ));

    // Idempotent no-op: unfavoriting again (not a favorite anymore) emits nothing.
    unmark_restaurant_as_favorite(
        &store,
        UnmarkRestaurantAsFavorite { customer_id: id, restaurant_id },
        &actor(),
    )
    .await
    .expect("no-op");
    assert_eq!(store.stream(&stream(id)).len(), 3, "no event emitted");
}

// ------------------------------------------------------------------------------------------------
// Address book & payment method (rules.yaml#/AddressBookManagement, #/PaymentMethodStorage)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCustomerAddressSet + TestCustomerAddressRemoved —
/// rules.yaml#/AddressBookManagement
#[tokio::test]
async fn saves_and_removes_an_address_and_ignores_removing_an_unknown_one() {
    let store = MemStore::default();
    let id = cust();
    let address_id = AddressId(uuid::Uuid::new_v4());
    store.seed(&stream(id), vec![registered_event(id)]);

    set_customer_address(
        &store,
        SetCustomerAddress {
            customer_id: id,
            address_id,
            label: Some("Home".into()),
            address: address(),
        },
        &actor(),
    )
    .await
    .expect("set address");
    let events = store.stream(&stream(id));
    assert!(matches!(
        &events[1],
        DomainEvent::CustomerAddressSet(e)
            if e.address_id == address_id && e.label.as_deref() == Some("Home")
    ));

    remove_customer_address(
        &store,
        RemoveCustomerAddress { customer_id: id, address_id },
        &actor(),
    )
    .await
    .expect("remove address");
    let events = store.stream(&stream(id));
    assert!(matches!(
        &events[2],
        DomainEvent::CustomerAddressRemoved(e) if e.address_id == address_id
    ));

    // Idempotent no-op: removing an unknown address emits nothing.
    remove_customer_address(
        &store,
        RemoveCustomerAddress { customer_id: id, address_id: AddressId(uuid::Uuid::new_v4()) },
        &actor(),
    )
    .await
    .expect("no-op");
    assert_eq!(store.stream(&stream(id)).len(), 3, "no event emitted");
}

/// tests.yaml#/cases/TestCustomerPaymentMethodSet — rules.yaml#/PaymentMethodStorage
#[tokio::test]
async fn sets_the_preferred_stripe_payment_method() {
    let store = MemStore::default();
    let id = cust();
    store.seed(&stream(id), vec![registered_event(id)]);

    set_customer_payment_method(
        &store,
        SetCustomerPaymentMethod { customer_id: id, payment_method_id: PaymentMethodId("pm_123".into()) },
        &actor(),
    )
    .await
    .expect("set payment method");

    let events = store.stream(&stream(id));
    assert!(matches!(
        &events[1],
        DomainEvent::CustomerPaymentMethodSet(e) if e.payment_method_id.0 == "pm_123"
    ));
}

/// A minimal projected restaurant row (only what the favorite-existence check reads).
fn minimal_restaurant_row(id: RestaurantId) -> RestaurantRow {
    RestaurantRow {
        restaurant_id: id,
        restaurant_account_id: None,
        listing_status: RestaurantListingStatus::ACTIVE_PARTNER,
        external_identifiers: None,
        google_place_id: None,
        slug: Slug("chez-marco".into()),
        display_name: RestaurantDisplayName("Chez Marco".into()),
        description: None,
        tags: None,
        margin_rate: None,
        cuisine_category: None,
        uber_prices_opt_in: None,
        website: None,
        rating: None,
        reviews_count: None,
        gbp_order_url: None,
        gbp_link_status: None,
        address: serde_json::json!({}),
        location: None,
        opening_hours: serde_json::json!([]),
        status: RestaurantStatus::ACTIVE,
        order_acceptance: OrderAcceptanceMode::NORMAL,
        default_currency: CurrencyCode("EUR".into()),
        timezone: None,
        preparation_time_minutes: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}
