//! BEHAVIOUR tests for the RestaurantAccount aggregate — the executable form of the `specs/tests.yaml`
//! Given/When/Then cases whose `when` is a RestaurantAccount command (ADR-0032: each test cites the
//! `specs/rules.yaml` rule it asserts). Given = pre-seeded stream events (in-memory event store),
//! When = the command handler, Then = the emitted event(s) / the errors.yaml rejection code.
//!
//! The RefAlreadyUsed arm of TestRestaurantAccountRegisterIsRejected is a documented
//! `TODO(invariant)` in `application::commands` (needs an external-reference index port) and is NOT
//! asserted here.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use application::commands::{
    delete_restaurant_account, register_restaurant_account, rejection_code,
    update_restaurant_account,
};
use application::ports::{version_conflict, Actor, EventStore};
use domain::generated::commands::{
    DeleteRestaurantAccount, RegisterRestaurantAccount, UpdateRestaurantAccount,
};
use domain::generated::entities::TaxRate;
use domain::generated::events::{DomainEvent, RestaurantAccountRegistered};
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

// ------------------------------------------------------------------------------------------------
// Fixtures (tests.yaml `fixtures`, with UUIDs instead of the sample string ids)
// ------------------------------------------------------------------------------------------------

fn actor() -> Actor {
    Actor {
        user_id: uuid::Uuid::new_v4(),
        user_type: 5, // UserType::ADMIN ordinal
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    }
}

fn stream(id: RestaurantAccountId) -> String {
    format!("RestaurantAccount-{}", id.0)
}

fn tax_rate() -> TaxRate {
    TaxRate { delivery: TaxRatePercent(10.0), collection: None, eat_in: None }
}

/// Fixture `restaurantAccountRegistered`.
fn registered_event(id: RestaurantAccountId) -> DomainEvent {
    DomainEvent::RestaurantAccountRegistered(RestaurantAccountRegistered {
        restaurant_account_id: id,
        r#ref: None,
        legal_name: RestaurantLegalName("SARL Chez Marco".into()),
        contact: None,
        default_currency: CurrencyCode("EUR".into()),
        default_tax_rate: tax_rate(),
        timezone: Some(TimeZone("Europe/Paris".into())),
    })
}

fn register_cmd(id: RestaurantAccountId, currency: &str) -> RegisterRestaurantAccount {
    RegisterRestaurantAccount {
        restaurant_account_id: id,
        legal_name: RestaurantLegalName("SARL Chez Marco".into()),
        contact: None,
        default_currency: CurrencyCode(currency.into()),
        default_tax_rate: tax_rate(),
        timezone: Some(TimeZone("Europe/Paris".into())),
        r#ref: None,
    }
}

fn empty_update(id: RestaurantAccountId) -> UpdateRestaurantAccount {
    UpdateRestaurantAccount {
        restaurant_account_id: id,
        legal_name: None,
        contact: None,
        default_tax_rate: None,
        timezone: None,
    }
}

fn aid() -> RestaurantAccountId {
    RestaurantAccountId(uuid::Uuid::new_v4())
}

// ------------------------------------------------------------------------------------------------
// Registration (rules.yaml#/AccountRegistrationValidCurrencyUniqueRef)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestRestaurantAccountRegistered —
/// rules.yaml#/AccountRegistrationValidCurrencyUniqueRef
#[tokio::test]
async fn registers_a_restaurant_account() {
    let store = MemStore::default();
    let id = aid();

    register_restaurant_account(&store, register_cmd(id, "EUR"), &actor())
        .await
        .expect("register");

    let events = store.stream(&stream(id));
    assert_eq!(events.len(), 1);
    let DomainEvent::RestaurantAccountRegistered(e) = &events[0] else {
        panic!("expected RestaurantAccountRegistered, got {:?}", events[0]);
    };
    assert_eq!(e.restaurant_account_id, id);
    assert_eq!(e.legal_name.0, "SARL Chez Marco");
    assert_eq!(e.default_currency.0, "EUR");
}

/// Idempotent replay: the version-0 clash on the stream is absorbed as success (client-generated
/// ids, ADR-0034).
#[tokio::test]
async fn replaying_the_same_registration_is_a_no_op() {
    let store = MemStore::default();
    let id = aid();
    store.seed(&stream(id), vec![registered_event(id)]);

    register_restaurant_account(&store, register_cmd(id, "EUR"), &actor())
        .await
        .expect("replay absorbed");
    assert_eq!(store.stream(&stream(id)).len(), 1, "no duplicate fact");
}

/// tests.yaml#/cases/TestRestaurantAccountRegisterIsRejected (InvalidCurrency arm) —
/// rules.yaml#/AccountRegistrationValidCurrencyUniqueRef. The RefAlreadyUsed arm is TODO(invariant)
/// until an external-reference index port exists.
#[tokio::test]
async fn rejects_registering_with_an_invalid_currency() {
    let store = MemStore::default();
    let id = aid();

    let err = register_restaurant_account(&store, register_cmd(id, "EURO"), &actor())
        .await
        .expect_err("EURO is not ISO 4217");
    assert_eq!(rejection_code(&err), Some("InvalidCurrency"));
    assert!(store.stream(&stream(id)).is_empty(), "no event on rejection");
}

// ------------------------------------------------------------------------------------------------
// Update (rules.yaml#/AccountUpdateRequiresExistingAccountAndField)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestRestaurantAccountUpdated —
/// rules.yaml#/AccountUpdateRequiresExistingAccountAndField
#[tokio::test]
async fn updates_account_level_fields() {
    let store = MemStore::default();
    let id = aid();
    store.seed(&stream(id), vec![registered_event(id)]);

    let mut cmd = empty_update(id);
    cmd.legal_name = Some(RestaurantLegalName("SARL Chez Marco II".into()));
    update_restaurant_account(&store, cmd, &actor()).await.expect("update");

    let events = store.stream(&stream(id));
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[1],
        DomainEvent::RestaurantAccountUpdated(e)
            if e.legal_name.as_ref().map(|n| n.0.as_str()) == Some("SARL Chez Marco II")
    ));
}

/// tests.yaml#/cases/TestRestaurantAccountUpdateIsRejected (both arms) —
/// rules.yaml#/AccountUpdateRequiresExistingAccountAndField
#[tokio::test]
async fn rejects_updating_a_missing_account_or_an_empty_update() {
    let store = MemStore::default();

    // Missing account → RestaurantAccountNotFound.
    let err = update_restaurant_account(&store, empty_update(aid()), &actor())
        .await
        .expect_err("missing account");
    assert_eq!(rejection_code(&err), Some("RestaurantAccountNotFound"));

    // Existing account, nothing editable provided → NoEditableFieldProvided.
    let id = aid();
    store.seed(&stream(id), vec![registered_event(id)]);
    let err = update_restaurant_account(&store, empty_update(id), &actor())
        .await
        .expect_err("empty update");
    assert_eq!(rejection_code(&err), Some("NoEditableFieldProvided"));
    assert_eq!(store.stream(&stream(id)).len(), 1, "no event on rejection");
}

// ------------------------------------------------------------------------------------------------
// Deletion (rules.yaml#/AccountDeletion)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestRestaurantAccountDeleted — rules.yaml#/AccountDeletion
#[tokio::test]
async fn deletes_a_restaurant_account() {
    let store = MemStore::default();
    let id = aid();
    store.seed(&stream(id), vec![registered_event(id)]);

    delete_restaurant_account(
        &store,
        DeleteRestaurantAccount { restaurant_account_id: id, reason: Some("Closed business".into()) },
        &actor(),
    )
    .await
    .expect("delete");

    let events = store.stream(&stream(id));
    assert!(matches!(
        &events[1],
        DomainEvent::RestaurantAccountDeleted(e) if e.reason.as_deref() == Some("Closed business")
    ));

    // A deleted account no longer exists: later commands reject with RestaurantAccountNotFound.
    let mut cmd = empty_update(id);
    cmd.legal_name = Some(RestaurantLegalName("Ghost".into()));
    let err = update_restaurant_account(&store, cmd, &actor()).await.expect_err("deleted");
    assert_eq!(rejection_code(&err), Some("RestaurantAccountNotFound"));

    // And deleting a never-registered account rejects too (actors.yaml throws).
    let err = delete_restaurant_account(
        &store,
        DeleteRestaurantAccount { restaurant_account_id: aid(), reason: None },
        &actor(),
    )
    .await
    .expect_err("missing");
    assert_eq!(rejection_code(&err), Some("RestaurantAccountNotFound"));
}
