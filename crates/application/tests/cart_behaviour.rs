//! BEHAVIOUR tests for the Cart aggregate — the executable form of the `specs/tests.yaml`
//! Given/When/Then cases whose `when` is a Cart-aggregate command (ADR-0032: each test cites the
//! `specs/rules.yaml` rule it asserts). Given = pre-seeded stream events (in-memory event store),
//! When = the command handler, Then = the emitted event(s) / the errors.yaml rejection code.
//!
//! Pure and offline: an in-memory [`EventStore`]. The live-catalog invariants
//! (OfferNotFound / OfferUnavailable / InsufficientStock / InvalidOptionSelection) still lack an
//! offer-level Catalog read port and are documented `TODO(invariant)`s in `application::commands` —
//! NOT asserted here. `CartNotFound` is unreachable for AddCartLine by construction
//! (create-on-first-add) and is asserted on remove/change instead.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use application::commands::{add_cart_line, change_cart_line_quantity, rejection_code, remove_cart_line};
use application::ports::{version_conflict, Actor, EventStore};
use domain::cart::MAX_LINE_QUANTITY;
use domain::generated::commands::{AddCartLine, CartLine, ChangeCartLineQuantity, RemoveCartLine};
use domain::generated::entities::CartLineItem;
use domain::generated::events::{
    CartCheckedOut, CartLineAdded, CartStarted, DomainEvent,
};
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
        user_type: 0, // UserType::PUBLIC ordinal — carts are built by (possibly guest) visitors
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    }
}

fn stream(id: CartId) -> String {
    format!("Cart-{}", id.0)
}

/// Fixture `cartStarted`.
fn cart_started(cart_id: CartId, restaurant_id: RestaurantId) -> DomainEvent {
    DomainEvent::CartStarted(CartStarted { cart_id, restaurant_id, customer_id: None })
}

/// Fixture `cartLineAdded` (line-1, offer off-1, quantity 2).
fn cart_line_added(cart_id: CartId, cart_line_id: CartLineId, offer_id: OfferId) -> DomainEvent {
    DomainEvent::CartLineAdded(CartLineAdded {
        cart_id,
        line: CartLineItem { cart_line_id, offer_id, quantity: 2, selected_option_ids: vec![] },
    })
}

/// Fixture `cartCheckedOut` — closes the cart (status CHECKED_OUT).
fn cart_checked_out(cart_id: CartId) -> DomainEvent {
    DomainEvent::CartCheckedOut(CartCheckedOut { cart_id, order_id: OrderId(uuid::Uuid::new_v4()) })
}

fn add_cmd(cart_id: CartId, restaurant_id: RestaurantId, line_id: CartLineId, quantity: i64) -> AddCartLine {
    AddCartLine {
        cart_id,
        restaurant_id,
        line: CartLine {
            cart_line_id: line_id,
            offer_id: OfferId(uuid::Uuid::new_v4()),
            quantity,
            selected_option_ids: vec![],
        },
    }
}

fn cid() -> CartId {
    CartId(uuid::Uuid::new_v4())
}
fn rid() -> RestaurantId {
    RestaurantId(uuid::Uuid::new_v4())
}
fn lid() -> CartLineId {
    CartLineId(uuid::Uuid::new_v4())
}

// ------------------------------------------------------------------------------------------------
// Adding lines (rules.yaml#/CartPricedFromLiveCatalog, #/CartRejectsUnorderableOrInvalidLine)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCartFirstLineAdded — rules.yaml#/CartPricedFromLiveCatalog
#[tokio::test]
async fn adds_the_first_line_creating_the_cart() {
    let store = MemStore::default();
    let (cart, resto, line) = (cid(), rid(), lid());

    add_cart_line(&store, add_cmd(cart, resto, line, 2), &actor()).await.expect("first add");

    let events = store.stream(&stream(cart));
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0],
        DomainEvent::CartStarted(e) if e.restaurant_id == resto && e.customer_id.is_none()
    ));
    assert!(matches!(
        &events[1],
        DomainEvent::CartLineAdded(e) if e.line.cart_line_id == line && e.line.quantity == 2
    ));
}

/// Client-generated line ids: re-sending a line the cart already holds is an idempotent replay
/// (no duplicate fact) — rules.yaml#/CartPricedFromLiveCatalog.
#[tokio::test]
async fn re_adding_the_same_line_is_a_no_op() {
    let store = MemStore::default();
    let (cart, resto, line) = (cid(), rid(), lid());
    let offer = OfferId(uuid::Uuid::new_v4());
    store.seed(&stream(cart), vec![cart_started(cart, resto), cart_line_added(cart, line, offer)]);

    let mut cmd = add_cmd(cart, resto, line, 2);
    cmd.line.offer_id = offer;
    add_cart_line(&store, cmd, &actor()).await.expect("replay absorbed");
    assert_eq!(store.stream(&stream(cart)).len(), 2, "no duplicate fact");
}

/// tests.yaml#/cases/TestCartAddLineIsRejectedWhenCartInvalid (CartNotOpen / CartRestaurantMismatch /
/// QuantityExceedsLimit arms) — rules.yaml#/CartRejectsUnorderableOrInvalidLine. The CartNotFound arm
/// is unreachable for AddCartLine (create-on-first-add); the catalog arms (OfferNotFound /
/// OfferUnavailable / InsufficientStock / InvalidOptionSelection, TestCartAddLineIsRejectedWhenOfferNotOrderable)
/// are TODO(invariant) until an offer-level Catalog read port exists.
#[tokio::test]
async fn rejects_adding_on_a_closed_or_mismatched_cart_or_over_the_limit() {
    let store = MemStore::default();

    // Checked-out cart → CartNotOpen.
    let (cart, resto) = (cid(), rid());
    store.seed(&stream(cart), vec![cart_started(cart, resto), cart_checked_out(cart)]);
    let err = add_cart_line(&store, add_cmd(cart, resto, lid(), 1), &actor()).await.expect_err("closed");
    assert_eq!(rejection_code(&err), Some("CartNotOpen"));

    // Another restaurant's line on an open cart → CartRestaurantMismatch (no mixing).
    let (cart, resto) = (cid(), rid());
    store.seed(&stream(cart), vec![cart_started(cart, resto)]);
    let err = add_cart_line(&store, add_cmd(cart, rid(), lid(), 1), &actor()).await.expect_err("mismatch");
    assert_eq!(rejection_code(&err), Some("CartRestaurantMismatch"));
    assert_eq!(store.stream(&stream(cart)).len(), 1, "no event on rejection");

    // Over the per-line cap → QuantityExceedsLimit.
    let err = add_cart_line(&store, add_cmd(cid(), rid(), lid(), MAX_LINE_QUANTITY + 1), &actor())
        .await
        .expect_err("over limit");
    assert_eq!(rejection_code(&err), Some("QuantityExceedsLimit"));
}

// ------------------------------------------------------------------------------------------------
// Changing / removing lines (rules.yaml#/CartPricedFromLiveCatalog,
// #/CartRejectsUnorderableOrInvalidLine)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCartLineQuantityChanged — rules.yaml#/CartPricedFromLiveCatalog
#[tokio::test]
async fn changes_the_quantity_of_an_existing_line() {
    let store = MemStore::default();
    let (cart, resto, line) = (cid(), rid(), lid());
    store.seed(&stream(cart), vec![cart_started(cart, resto), cart_line_added(cart, line, OfferId(uuid::Uuid::new_v4()))]);

    change_cart_line_quantity(
        &store,
        ChangeCartLineQuantity { cart_id: cart, cart_line_id: line, quantity: 3 },
        &actor(),
    )
    .await
    .expect("change quantity");

    let events = store.stream(&stream(cart));
    assert_eq!(events.len(), 3);
    assert!(matches!(
        &events[2],
        DomainEvent::CartLineQuantityChanged(e) if e.cart_line_id == line && e.quantity == 3
    ));
}

/// tests.yaml#/cases/TestCartLineRemoved — rules.yaml#/CartPricedFromLiveCatalog
#[tokio::test]
async fn removes_a_line_from_the_cart() {
    let store = MemStore::default();
    let (cart, resto, line) = (cid(), rid(), lid());
    store.seed(&stream(cart), vec![cart_started(cart, resto), cart_line_added(cart, line, OfferId(uuid::Uuid::new_v4()))]);

    remove_cart_line(&store, RemoveCartLine { cart_id: cart, cart_line_id: line }, &actor())
        .await
        .expect("remove line");

    let events = store.stream(&stream(cart));
    assert!(matches!(&events[2], DomainEvent::CartLineRemoved(e) if e.cart_line_id == line));
}

/// tests.yaml#/cases/TestCartRemoveLineIsRejected (all three arms) —
/// rules.yaml#/CartRejectsUnorderableOrInvalidLine
#[tokio::test]
async fn rejects_removing_from_a_missing_or_closed_cart_or_a_missing_line() {
    let store = MemStore::default();

    // Missing cart → CartNotFound.
    let err = remove_cart_line(&store, RemoveCartLine { cart_id: cid(), cart_line_id: lid() }, &actor())
        .await
        .expect_err("missing cart");
    assert_eq!(rejection_code(&err), Some("CartNotFound"));

    // Checked-out cart → CartNotOpen.
    let (cart, resto, line) = (cid(), rid(), lid());
    store.seed(
        &stream(cart),
        vec![cart_started(cart, resto), cart_line_added(cart, line, OfferId(uuid::Uuid::new_v4())), cart_checked_out(cart)],
    );
    let err = remove_cart_line(&store, RemoveCartLine { cart_id: cart, cart_line_id: line }, &actor())
        .await
        .expect_err("closed cart");
    assert_eq!(rejection_code(&err), Some("CartNotOpen"));

    // Open cart, unknown line → CartLineNotFound.
    let (cart, resto) = (cid(), rid());
    store.seed(&stream(cart), vec![cart_started(cart, resto)]);
    let err = remove_cart_line(&store, RemoveCartLine { cart_id: cart, cart_line_id: lid() }, &actor())
        .await
        .expect_err("missing line");
    assert_eq!(rejection_code(&err), Some("CartLineNotFound"));
    assert_eq!(store.stream(&stream(cart)).len(), 1, "no event on rejection");
}

/// ChangeCartLineQuantity rejection arms (actors.yaml throws: CartNotFound / CartLineNotFound /
/// QuantityExceedsLimit; InsufficientStock is TODO(invariant)) —
/// rules.yaml#/CartRejectsUnorderableOrInvalidLine
#[tokio::test]
async fn rejects_changing_quantity_on_invalid_cart_line_or_over_the_limit() {
    let store = MemStore::default();

    let err = change_cart_line_quantity(
        &store,
        ChangeCartLineQuantity { cart_id: cid(), cart_line_id: lid(), quantity: 1 },
        &actor(),
    )
    .await
    .expect_err("missing cart");
    assert_eq!(rejection_code(&err), Some("CartNotFound"));

    let (cart, resto, line) = (cid(), rid(), lid());
    store.seed(&stream(cart), vec![cart_started(cart, resto), cart_line_added(cart, line, OfferId(uuid::Uuid::new_v4()))]);

    let err = change_cart_line_quantity(
        &store,
        ChangeCartLineQuantity { cart_id: cart, cart_line_id: lid(), quantity: 1 },
        &actor(),
    )
    .await
    .expect_err("missing line");
    assert_eq!(rejection_code(&err), Some("CartLineNotFound"));

    let err = change_cart_line_quantity(
        &store,
        ChangeCartLineQuantity { cart_id: cart, cart_line_id: line, quantity: MAX_LINE_QUANTITY + 1 },
        &actor(),
    )
    .await
    .expect_err("over limit");
    assert_eq!(rejection_code(&err), Some("QuantityExceedsLimit"));
    assert_eq!(store.stream(&stream(cart)).len(), 2, "no event on rejection");
}
