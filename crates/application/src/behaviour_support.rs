//! HARNESS RUNTIME for the GENERATED behaviour-test suite (issue #24, ADR pending).
//!
//! The suite itself is GENERATED from `specs/tests.yaml` into `generated/behaviour_tests.rs`
//! (one `#[tokio::test]` per Given/When/Then case). This module is the hand-written runtime the
//! generated tests run on: the in-memory event store, the read-model / service test doubles, the
//! deterministic spec-id → UUID mapping, and the seed/diff/assert helpers. Playbook rule: when a
//! behaviour test fails, fix THIS runtime or the emitter — never the spec or the generated test.
//!
//! Conventions mirrored from the hand-written suite this generated one replaces:
//! - stream keys are `<Category>-<id>` (the `Aggregate::stream` convention);
//! - spec string ids ("order-1") become deterministic UUIDs (`uid`, UUIDv5) — EXCEPT delivery-job
//!   ids, which mirror the dispatch PM's own derivation (`delivery_job_id_for`) so payload
//!   equality holds on PM-emitted `DeliveryRequested` facts;
//! - the Stripe gateway double answers `pi_123`/`pi_123_secret` and declines exactly the
//!   `pm_declined` payment method;
//! - the identity double resolves any OTP-verified phone to the spec's `auth-supabase-1` and
//!   rejects the canonical bad code `000000`.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use domain::generated::entities::Money;
use domain::generated::events::DomainEvent;
use domain::generated::scalars::*;
use domain::shared::errors::DomainError;

use crate::generated::services::{
    DeliveryOfferJobInput, DeliveryService, IdentitySendEmailMagicLinkInput,
    IdentitySendPhoneOtpInput, IdentityService, IdentityVerifyEmailTokenInput,
    IdentityVerifyEmailTokenOutput, IdentityVerifyPhoneOtpInput, IdentityVerifyPhoneOtpOutput,
    PaymentRefundInput, PaymentRequestInput, PaymentRequestOutput, PaymentService, ServiceCallMeta,
};
use crate::pm_state::mem::{
    MemCartBindingState, MemDeliveryDispatchState, MemPaymentProcessState, MemRefundProcessState,
};
use crate::pm_state::{
    DeliveryDispatchRow, DeliveryDispatchStateStore, PaymentProcessRow,
    PaymentProcessStateStore, RefundProcessRow, RefundProcessStateStore,
};
use crate::ports::{Actor, EventStore, GbpOrderLinkProbe, GoogleOwnershipVerifier};
use crate::process_managers::test_support::MemStore;
use crate::queries::{
    CartReadRepository, CatalogReadRepository, CustomerReadRepository, OfferView, OrderFilter,
    OrderReadRepository, ProspectFilter, ProspectionReadRepository, RestaurantFilter,
    RestaurantReadRepository,
};
use crate::repository::Repository;

pub use crate::process_managers::test_support::envelope;

// ------------------------------------------------------------------------------------------------
// Deterministic ids
// ------------------------------------------------------------------------------------------------

/// Spec string id → deterministic UUID (v5 over the literal), stable across runs and processes.
///
/// Exception: the spec's delivery-job ids. The dispatch PM derives the job id FROM the order id
/// (`delivery_job_id_for`, the run's idempotency key), so the spec pair `deliv-1`/`order-1` must
/// resolve to that same derivation or payload equality on `DeliveryRequested` could never hold.
pub fn uid(s: &str) -> uuid::Uuid {
    match s {
        "deliv-1" => {
            crate::process_managers::delivery_dispatch::delivery_job_id_for(&OrderId(uid("order-1"))).0
        }
        _ => uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, s.as_bytes()),
    }
}

/// The fixed command-side actor (ADMIN-level envelope identity; the envelope is not asserted).
pub fn actor() -> Actor {
    actor_as(5)
}

/// A command-side actor with an explicit UserType ORDINAL — for the handlers whose semantics
/// derive from the acting persona (e.g. `TipOrder`'s `tippedBy`, ADR-0041).
pub fn actor_as(user_type: i32) -> Actor {
    Actor {
        user_id: uuid::Uuid::from_u128(0xA0),
        user_type,
        correlation_id: uuid::Uuid::from_u128(0xC0),
        cause_id: None,
    }
}

// ------------------------------------------------------------------------------------------------
// The bed: event store + PM state doubles + read-model/service doubles
// ------------------------------------------------------------------------------------------------

/// Everything a generated behaviour test dispatches against.
#[derive(Default)]
pub struct TestBed {
    pub store: MemStore,
    pub payment_pm: MemPaymentProcessState,
    pub refund_pm: MemRefundProcessState,
    pub cart_pm: MemCartBindingState,
    pub dispatch_pm: MemDeliveryDispatchState,
    pub restaurants: SpecRestaurants,
    pub catalogs: SpecCatalogs,
    pub carts: SpecCarts,
    pub customers: SpecCustomers,
    pub orders: SpecOrders,
    pub prospection: SpecProspection,
    pub payments: FakeGateway,
    pub delivery: FakeDelivery,
    pub identity: FakeIdentity,
    pub ownership: FakeOwnership,
    pub probe: FakeProbe,
}

/// Stream lengths before the WHEN — the diff baseline.
pub type Snapshot = HashMap<String, usize>;

impl TestBed {
    pub fn new() -> Self {
        Self::default()
    }

    /// GIVEN: seed already-recorded facts onto `stream` and mirror their read-model / PM-run
    /// effects (what projections and saga legs would have materialized when they were recorded).
    pub async fn seed(&self, stream: &str, events: Vec<DomainEvent>) {
        for event in &events {
            self.apply_effects(event).await;
        }
        let mut all = self.store.stream(stream);
        all.extend(events);
        self.store.seed(stream, all);
    }

    /// Stream lengths now (call between GIVEN and WHEN).
    pub fn snapshot(&self) -> Snapshot {
        self.store.lengths()
    }

    /// THEN: the exact facts appended since `before`, grouped per stream in append order, must
    /// equal `expected` — nothing more, nothing less, on any stream (`expected == []` asserts a
    /// strict no-op).
    pub fn assert_appended(&self, case: &str, before: &Snapshot, expected: &[(String, DomainEvent)]) {
        let mut want: HashMap<&str, Vec<&DomainEvent>> = HashMap::new();
        for (stream, event) in expected {
            want.entry(stream.as_str()).or_default().push(event);
        }
        let after = self.store.lengths();
        for (stream, len) in &after {
            let base = before.get(stream).copied().unwrap_or(0);
            let appended = &self.store.stream(stream)[base..*len];
            let expected_here = want.remove(stream.as_str()).unwrap_or_default();
            assert_eq!(
                appended.iter().collect::<Vec<_>>(),
                expected_here,
                "{case}: stream `{stream}` appended facts differ from the spec's `then`"
            );
        }
        assert!(
            want.is_empty(),
            "{case}: spec expected appends on streams that saw none: {:?}",
            want.keys().collect::<Vec<_>>()
        );
    }

    /// WHEN (aggregate ← delivered/inbound EVENT with no standalone handler): record the fact on
    /// its stream through the write path (Repository + optimistic append), idempotent by
    /// structural equality — the same semantics `record_inbound_payment_event` and the PM deliver
    /// legs use. The production inbound-events drain (ADR-20260720-015300) will subsume this.
    pub async fn record_fact(&self, stream: &str, event: DomainEvent) -> Result<(), DomainError> {
        let (events, version) = self.store.load(stream).await?;
        if events.iter().any(|e| e == &event) {
            return Ok(());
        }
        Repository::new(&self.store).save(stream, version, &[event], &actor()).await.map(|_| ())
    }

    /// Read-model + PM-run effects of one already-recorded GIVEN fact. Mirrors what the
    /// projectors / saga legs materialized when the fact was first recorded; extend as specs grow.
    async fn apply_effects(&self, event: &DomainEvent) {
        match event {
            // --- Prospection read model ----------------------------------------------------
            DomainEvent::ProspectContacted(e) => {
                self.prospection.record_contact(e.restaurant_id);
            }
            // --- Restaurant read model -----------------------------------------------------
            DomainEvent::RestaurantRegistered(e) => {
                self.restaurants.upsert(restaurant_row_from_registered(e));
            }
            DomainEvent::RestaurantActivated(e) => {
                self.restaurants.set_status(e.restaurant_id, RestaurantStatus::ACTIVE);
            }
            DomainEvent::RestaurantDeactivated(e) => {
                self.restaurants.set_status(e.restaurant_id, RestaurantStatus::INACTIVE);
            }
            // --- Catalog read model --------------------------------------------------------
            DomainEvent::ProductAdded(e) => {
                for offer in &e.product.offers {
                    self.catalogs.add_offer(
                        e.restaurant_id,
                        offer_view(&e.product.name.0, offer),
                    );
                }
            }
            DomainEvent::CatalogImported(e) => {
                for product in &e.products {
                    for offer in &product.offers {
                        self.catalogs.add_offer(e.restaurant_id, offer_view(&product.name.0, offer));
                    }
                }
            }
            DomainEvent::OfferStockUpdated(e) => {
                self.catalogs.set_stock(e.offer_id, e.stock.status, Some(e.stock.quantity));
            }
            // --- Cart read model -----------------------------------------------------------
            DomainEvent::CartStarted(e) => {
                self.carts.upsert(crate::queries::CartRow {
                    cart_id: e.cart_id,
                    restaurant_id: e.restaurant_id,
                    session_id: e.session_id.clone(),
                    customer_id: None,
                    status: CartStatus::OPEN,
                    lines: serde_json::json!([]),
                    total_amount_cents: MoneyCents(0),
                    currency: CurrencyCode("EUR".into()),
                    estimated_breakdown: None,
                    uber_comparison: None,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                });
            }
            DomainEvent::CartBoundToCustomer(e) => {
                self.carts.bind(e.cart_id, e.customer_id);
            }
            DomainEvent::CartCheckedOut(e) => {
                self.carts.set_status(e.cart_id, CartStatus::CHECKED_OUT);
            }
            // --- Customer read model -------------------------------------------------------
            DomainEvent::CustomerRegistered(e) => {
                self.customers.upsert(crate::queries::CustomerRow {
                    customer_id: e.customer_id,
                    phone: e.phone.clone(),
                    auth_ref: e.auth_ref.clone(),
                    display_name: e.display_name.clone(),
                    email: e.email.clone(),
                    email_verified: false,
                    locale: e.locale.clone(),
                    timezone: e.timezone.clone(),
                    ratings: serde_json::json!([]),
                    favorite_restaurant_ids: serde_json::json!([]),
                    preferences: None,
                    addresses: serde_json::json!([]),
                    payment_method_id: None,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                });
            }
            DomainEvent::CustomerEmailVerified(e) => {
                self.customers.set_email(e.customer_id, e.email.clone());
            }
            DomainEvent::CustomerPhoneChanged(e) => {
                self.customers.set_phone(e.customer_id, e.phone.clone());
            }
            // --- Order read model + payment PM run -----------------------------------------
            DomainEvent::OrderPlaced(e) => {
                self.orders.upsert(tracking_row_from_order_placed(e));
            }
            DomainEvent::OrderMarkedReady(e) => {
                self.orders.set_status(e.order_id, OrderStatus::READY);
            }
            DomainEvent::PaymentIntentCreated(e) => {
                self.orders.set_payment(e.checkout.order_id, "PENDING", &e.payment_intent_id);
                self.payment_pm
                    .upsert(&PaymentProcessRow {
                        cart_id: e.checkout.cart_id,
                        order_id: e.checkout.order_id,
                        payment_intent_id: e.payment_intent_id.clone(),
                        process_status: PaymentProcessStatus::AWAITING_PAYMENT_RESULT,
                        payment_status: PaymentStatus::PENDING,
                        customer_id: e.checkout.customer_id,
                        session_id: None,
                        client_secret: Some("pi_123_secret".into()),
                        last_processed_stripe_event_id: None,
                        last_update_utc: chrono::Utc::now(),
                    })
                    .await
                    .expect("seed payment run");
            }
            DomainEvent::PaymentCaptured(e) => {
                if let Some(order_id) = e.order_id {
                    self.orders.set_payment(order_id, "CAPTURED", &e.payment_intent_id);
                }
                self.orders.set_payment_by_intent("CAPTURED", &e.payment_intent_id);
            }
            // --- Refund PM run -------------------------------------------------------------
            DomainEvent::RefundRequested(e) => {
                self.seed_refund_run(e.order_id, e.reason.clone()).await;
            }
            DomainEvent::RefundOpened(e) => {
                self.seed_refund_run(e.order_id, e.reason.clone()).await;
            }
            // --- Delivery dispatch PM run --------------------------------------------------
            DomainEvent::DeliveryRequested(e) => {
                self.dispatch_pm
                    .upsert(&DeliveryDispatchRow {
                        order_id: e.order_id,
                        restaurant_id: e.restaurant_id,
                        delivery_job_id: e.delivery_job_id,
                        process_status: DeliveryDispatchProcessStatus::OFFERED,
                        offer_attempts: 1,
                        last_update_utc: chrono::Utc::now(),
                    })
                    .await
                    .expect("seed dispatch run");
            }
            DomainEvent::DeliveryRejectedByPartner(e) => {
                if let Some(row) = self
                    .dispatch_pm
                    .by_delivery_job(e.delivery_job_id)
                    .await
                    .expect("dispatch run lookup")
                {
                    self.dispatch_pm
                        .upsert(&DeliveryDispatchRow {
                            offer_attempts: row.offer_attempts + 1,
                            ..row
                        })
                        .await
                        .expect("seed dispatch re-offer");
                }
            }
            DomainEvent::DeliveryAcceptedByPartner(e) => {
                self.set_dispatch_status(e.delivery_job_id, DeliveryDispatchProcessStatus::ACCEPTED)
                    .await;
            }
            DomainEvent::DeliveryAcceptedByRider(e) => {
                self.set_dispatch_status(e.delivery_job_id, DeliveryDispatchProcessStatus::ACCEPTED)
                    .await;
            }
            _ => {}
        }
    }

    async fn set_dispatch_status(&self, job: DeliveryJobId, status: DeliveryDispatchProcessStatus) {
        if let Some(row) = self.dispatch_pm.by_delivery_job(job).await.expect("dispatch run lookup") {
            self.dispatch_pm
                .upsert(&DeliveryDispatchRow { process_status: status, ..row })
                .await
                .expect("seed dispatch status");
        }
    }

    async fn seed_refund_run(&self, order_id: OrderId, reason: Option<String>) {
        let intent = self
            .orders
            .by_id_sync(order_id)
            .and_then(|row| row.payment_intent_id)
            .or(Some(PaymentIntentId("pi_123".into())));
        self.refund_pm
            .upsert(&RefundProcessRow {
                order_id,
                payment_intent_id: intent,
                refund_id: None,
                process_status: RefundProcessStatus::PENDING_APPROVAL,
                approved_amount_cents: None,
                reason,
                last_update_utc: chrono::Utc::now(),
            })
            .await
            .expect("seed refund run");
    }
}

/// Register every offer a catalog-content fact carries into the catalog read-model double —
/// the generated `spec_baseline` calls this for the fixture pool's `ProductAdded` /
/// `CatalogImported` facts so pricing (`offer_by_id`) answers like the projected catalog would.
pub fn install_catalog_offers(bed: &TestBed, event: &DomainEvent) {
    match event {
        DomainEvent::ProductAdded(e) => {
            for offer in &e.product.offers {
                bed.catalogs.add_offer(e.restaurant_id, offer_view(&e.product.name.0, offer));
            }
        }
        DomainEvent::CatalogImported(e) => {
            for product in &e.products {
                for offer in &product.offers {
                    bed.catalogs.add_offer(e.restaurant_id, offer_view(&product.name.0, offer));
                }
            }
        }
        _ => {}
    }
}

/// `thrown`: the rejection code must be ONE OF the codes the spec lists for the scenario (a
/// `thrown` list bundles the errors that can apply to one rejection; the sample data triggers one).
pub fn assert_thrown(case: &str, err: &DomainError, codes: &[&str]) {
    let code = crate::commands::rejection_code(err);
    assert!(
        code.map(|c| codes.contains(&c)).unwrap_or(false),
        "{case}: rejected with {err:?}, expected one of {codes:?}"
    );
}

// ------------------------------------------------------------------------------------------------
// Row builders (spec payload → read-model row, inert columns defaulted)
// ------------------------------------------------------------------------------------------------

fn restaurant_row_from_registered(
    e: &domain::generated::events::RestaurantRegistered,
) -> crate::queries::RestaurantRow {
    crate::queries::RestaurantRow {
        restaurant_id: e.restaurant_id,
        restaurant_account_id: e.account_id,
        listing_status: e.listing_status,
        external_identifiers: None,
        google_place_id: None,
        slug: e.slug.clone(),
        display_name: e.display_name.clone(),
        description: None,
        tags: None,
        margin_rate: e.margin_rate,
        cuisine_category: e.cuisine_category,
        uber_prices_opt_in: e.uber_prices_opt_in,
        website: e.website.clone(),
        rating: None,
        reviews_count: None,
        gbp_order_url: None,
        gbp_link_status: None,
        address: serde_json::to_value(&e.address).expect("address json"),
        location: None,
        opening_hours: serde_json::json!([]),
        status: RestaurantStatus::INACTIVE,
        order_acceptance: OrderAcceptanceMode::NORMAL,
        default_currency: CurrencyCode("EUR".into()),
        timezone: e.timezone.clone(),
        preparation_time_minutes: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

pub fn tracking_row_from_order_placed(
    e: &domain::generated::events::OrderPlaced,
) -> crate::queries::OrderTrackingRow {
    crate::queries::OrderTrackingRow {
        order_id: e.order_id,
        r#ref: ExternalReference(format!("spec-{}", e.order_id.0)),
        restaurant_id: e.restaurant_id,
        customer_id: e.customer_id,
        status: OrderStatus::PLACED,
        service_type: e.service_type,
        items: serde_json::to_value(&e.items).expect("items json"),
        total_amount_cents: e.total_amount.amount_cents,
        currency: e.total_amount.currency.clone(),
        articles_cents: e.breakdown.articles.amount_cents,
        delivery_cents: e.breakdown.delivery.amount_cents,
        service_fee_cents: e.breakdown.service_fee.amount_cents,
        restaurant_payout_cents: e.breakdown.restaurant_payout.amount_cents,
        rider_payout_cents: e.breakdown.rider_payout.amount_cents,
        captain_net_cents: e.breakdown.captain_net.amount_cents,
        uber_total_cents: None,
        uber_restaurant_cents: None,
        uber_rider_cents: None,
        uber_platform_cents: None,
        uber_basis: None,
        delivery_address: e
            .delivery_address
            .as_ref()
            .map(|a| serde_json::to_value(a).expect("address json")),
        estimated_ready_at: None,
        placed_at: chrono::Utc::now(),
        status_changed_at: chrono::Utc::now(),
        payment_intent_id: Some(e.payment_intent_id.clone()),
        payment_status: "CAPTURED".to_string(),
        restaurant_stars: None,
        rating_comment: None,
        rider_thumb: None,
        rider_tip_cents: None,
        restaurant_tip_cents: None,
        captain_tip_cents: None,
        rated_at: None,
        delivery_status: None,
        courier: None,
        estimated_dropoff_at: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn offer_view(product_name: &str, offer: &domain::generated::entities::Offer) -> OfferView {
    OfferView {
        offer_id: offer.id,
        product_id: offer.product_id,
        product_name: ProductName(product_name.to_string()),
        offer_name: offer.name.clone(),
        price: offer.price.clone(),
        availability: offer.availability,
        stock_status: offer.stock.as_ref().map(|s| s.status).unwrap_or(StockStatus::IN_STOCK),
        stock_quantity: offer.stock.as_ref().map(|s| s.quantity),
        option_lists: Vec::new(),
    }
}

// ------------------------------------------------------------------------------------------------
// Read-model doubles (Mutex<Vec/HashMap> rows, answering like the Pg adapters would)
// ------------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct SpecRestaurants {
    rows: Mutex<Vec<crate::queries::RestaurantRow>>,
}

impl SpecRestaurants {
    fn upsert(&self, row: crate::queries::RestaurantRow) {
        let mut rows = self.rows.lock().unwrap();
        rows.retain(|r| r.restaurant_id != row.restaurant_id);
        rows.push(row);
    }
    fn set_status(&self, id: RestaurantId, status: RestaurantStatus) {
        let mut rows = self.rows.lock().unwrap();
        if let Some(row) = rows.iter_mut().find(|r| r.restaurant_id == id) {
            row.status = status;
        }
    }
}

#[async_trait]
impl RestaurantReadRepository for SpecRestaurants {
    async fn list(
        &self,
        _filter: RestaurantFilter,
    ) -> Result<Vec<crate::queries::RestaurantRow>, DomainError> {
        Ok(self.rows.lock().unwrap().clone())
    }
    async fn by_slug(&self, slug: Slug) -> Result<Option<crate::queries::RestaurantRow>, DomainError> {
        Ok(self.rows.lock().unwrap().iter().find(|r| r.slug == slug).cloned())
    }
    async fn by_id(&self, id: RestaurantId) -> Result<Option<crate::queries::RestaurantRow>, DomainError> {
        Ok(self.rows.lock().unwrap().iter().find(|r| r.restaurant_id == id).cloned())
    }
    async fn by_account(
        &self,
        account_id: RestaurantAccountId,
    ) -> Result<Vec<crate::queries::RestaurantRow>, DomainError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.restaurant_account_id == Some(account_id))
            .cloned()
            .collect())
    }
}

#[derive(Default)]
pub struct SpecCatalogs {
    offers: Mutex<Vec<(RestaurantId, OfferView)>>,
}

impl SpecCatalogs {
    pub fn add_offer(&self, restaurant_id: RestaurantId, view: OfferView) {
        let mut offers = self.offers.lock().unwrap();
        offers.retain(|(_, o)| o.offer_id != view.offer_id);
        offers.push((restaurant_id, view));
    }
    fn set_stock(&self, offer_id: OfferId, status: StockStatus, quantity: Option<Quantity>) {
        let mut offers = self.offers.lock().unwrap();
        if let Some((_, view)) = offers.iter_mut().find(|(_, o)| o.offer_id == offer_id) {
            view.stock_status = status;
            view.stock_quantity = quantity;
        }
    }
}

#[async_trait]
impl CatalogReadRepository for SpecCatalogs {
    async fn by_restaurant(
        &self,
        _restaurant_id: RestaurantId,
    ) -> Result<Option<crate::queries::CatalogRow>, DomainError> {
        Ok(None)
    }
    async fn offer_by_id(
        &self,
        restaurant_id: RestaurantId,
        offer_id: OfferId,
    ) -> Result<Option<OfferView>, DomainError> {
        Ok(self
            .offers
            .lock()
            .unwrap()
            .iter()
            .find(|(rid, o)| *rid == restaurant_id && o.offer_id == offer_id)
            .map(|(_, o)| o.clone()))
    }
}

#[derive(Default)]
pub struct SpecCarts {
    rows: Mutex<Vec<crate::queries::CartRow>>,
}

impl SpecCarts {
    fn upsert(&self, row: crate::queries::CartRow) {
        let mut rows = self.rows.lock().unwrap();
        rows.retain(|r| r.cart_id != row.cart_id);
        rows.push(row);
    }
    fn bind(&self, id: CartId, customer: CustomerId) {
        let mut rows = self.rows.lock().unwrap();
        if let Some(row) = rows.iter_mut().find(|r| r.cart_id == id) {
            row.customer_id = Some(customer);
        }
    }
    fn set_status(&self, id: CartId, status: CartStatus) {
        let mut rows = self.rows.lock().unwrap();
        if let Some(row) = rows.iter_mut().find(|r| r.cart_id == id) {
            row.status = status;
        }
    }
}

#[async_trait]
impl CartReadRepository for SpecCarts {
    async fn by_customer(&self, customer_id: CustomerId) -> Result<Vec<crate::queries::CartRow>, DomainError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.customer_id == Some(customer_id))
            .cloned()
            .collect())
    }
    async fn by_id(&self, id: CartId) -> Result<Option<crate::queries::CartRow>, DomainError> {
        Ok(self.rows.lock().unwrap().iter().find(|r| r.cart_id == id).cloned())
    }
    async fn open_by_session(&self, session_id: SessionId) -> Result<Vec<crate::queries::CartRow>, DomainError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.session_id == session_id && r.status == CartStatus::OPEN)
            .cloned()
            .collect())
    }
}

#[derive(Default)]
pub struct SpecCustomers {
    rows: Mutex<Vec<crate::queries::CustomerRow>>,
}

impl SpecCustomers {
    fn upsert(&self, row: crate::queries::CustomerRow) {
        let mut rows = self.rows.lock().unwrap();
        rows.retain(|r| r.customer_id != row.customer_id);
        rows.push(row);
    }
    fn set_email(&self, id: CustomerId, email: EmailAddress) {
        let mut rows = self.rows.lock().unwrap();
        if let Some(row) = rows.iter_mut().find(|r| r.customer_id == id) {
            row.email = Some(email);
            row.email_verified = true;
        }
    }
    fn set_phone(&self, id: CustomerId, phone: PhoneNumber) {
        let mut rows = self.rows.lock().unwrap();
        if let Some(row) = rows.iter_mut().find(|r| r.customer_id == id) {
            row.phone = phone;
        }
    }
}

/// The spec's canonical ALREADY-TAKEN identities: `+33600000000` / `taken@example.com` belong to a
/// customer that is not part of any GIVEN — the duplicate-rejection samples assume them.
fn other_customer() -> crate::queries::CustomerRow {
    crate::queries::CustomerRow {
        customer_id: CustomerId(uid("cust-other")),
        phone: PhoneNumber("+33600000000".into()),
        auth_ref: None,
        display_name: None,
        email: Some(EmailAddress("taken@example.com".into())),
        email_verified: true,
        locale: None,
        timezone: None,
        ratings: serde_json::json!([]),
        favorite_restaurant_ids: serde_json::json!([]),
        preferences: None,
        addresses: serde_json::json!([]),
        payment_method_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

#[async_trait]
impl CustomerReadRepository for SpecCustomers {
    async fn by_phone(&self, phone: PhoneNumber) -> Result<Option<crate::queries::CustomerRow>, DomainError> {
        if phone.0 == "+33600000000" {
            return Ok(Some(other_customer()));
        }
        Ok(self.rows.lock().unwrap().iter().find(|r| r.phone == phone).cloned())
    }
    async fn by_email(&self, email: EmailAddress) -> Result<Option<crate::queries::CustomerRow>, DomainError> {
        if email.0 == "taken@example.com" {
            return Ok(Some(other_customer()));
        }
        Ok(self.rows.lock().unwrap().iter().find(|r| r.email.as_ref() == Some(&email)).cloned())
    }
    async fn by_id(&self, id: CustomerId) -> Result<Option<crate::queries::CustomerRow>, DomainError> {
        Ok(self.rows.lock().unwrap().iter().find(|r| r.customer_id == id).cloned())
    }
    async fn by_auth_ref(
        &self,
        auth_ref: ExternalReference,
    ) -> Result<Option<crate::queries::CustomerRow>, DomainError> {
        Ok(self.rows.lock().unwrap().iter().find(|r| r.auth_ref.as_ref() == Some(&auth_ref)).cloned())
    }
}

#[derive(Default)]
pub struct SpecOrders {
    rows: Mutex<Vec<crate::queries::OrderTrackingRow>>,
}

impl SpecOrders {
    pub fn upsert(&self, row: crate::queries::OrderTrackingRow) {
        let mut rows = self.rows.lock().unwrap();
        rows.retain(|r| r.order_id != row.order_id);
        rows.push(row);
    }
    fn set_status(&self, id: OrderId, status: OrderStatus) {
        let mut rows = self.rows.lock().unwrap();
        if let Some(row) = rows.iter_mut().find(|r| r.order_id == id) {
            row.status = status;
        }
    }
    fn set_payment(&self, id: OrderId, status: &str, intent: &PaymentIntentId) {
        let mut rows = self.rows.lock().unwrap();
        if let Some(row) = rows.iter_mut().find(|r| r.order_id == id) {
            row.payment_status = status.to_string();
            row.payment_intent_id = Some(intent.clone());
        }
    }
    fn set_payment_by_intent(&self, status: &str, intent: &PaymentIntentId) {
        let mut rows = self.rows.lock().unwrap();
        if let Some(row) =
            rows.iter_mut().find(|r| r.payment_intent_id.as_ref() == Some(intent))
        {
            row.payment_status = status.to_string();
        }
    }
    fn by_id_sync(&self, id: OrderId) -> Option<crate::queries::OrderTrackingRow> {
        self.rows.lock().unwrap().iter().find(|r| r.order_id == id).cloned()
    }
}

#[async_trait]
impl OrderReadRepository for SpecOrders {
    async fn list(&self, filter: OrderFilter) -> Result<Vec<crate::queries::OrderTrackingRow>, DomainError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|r| filter.customer_id.map(|c| r.customer_id == Some(c)).unwrap_or(true))
            .filter(|r| filter.restaurant_id.map(|x| r.restaurant_id == x).unwrap_or(true))
            .filter(|r| filter.status.map(|s| r.status == s).unwrap_or(true))
            .cloned()
            .collect())
    }
    async fn by_id(&self, id: OrderId) -> Result<Option<crate::queries::OrderTrackingRow>, DomainError> {
        Ok(self.by_id_sync(id))
    }
}

/// Prospection pipeline double: rows materialize from seeded `ProspectContacted` facts with
/// `last_contacted_at = now` — the anti-spam window check reads the projection, and a GIVEN
/// contact is by definition a recent one (the spec has no relance-elapsed case to contradict it).
#[derive(Default)]
pub struct SpecProspection {
    rows: Mutex<Vec<crate::queries::ProspectionPipelineRow>>,
}

impl SpecProspection {
    fn record_contact(&self, restaurant_id: RestaurantId) {
        let mut rows = self.rows.lock().unwrap();
        if let Some(row) = rows.iter_mut().find(|r| r.restaurant_id == restaurant_id) {
            row.contacts_count += 1;
            row.last_contacted_at = Some(chrono::Utc::now());
        } else {
            rows.push(crate::queries::ProspectionPipelineRow {
                restaurant_id,
                score: ProspectionScore(0),
                pipeline_status: ProspectPipelineStatus::CONTACTED,
                contacts_count: 1,
                last_contacted_at: Some(chrono::Utc::now()),
                replied_at: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            });
        }
    }
}

#[async_trait]
impl ProspectionReadRepository for SpecProspection {
    async fn list(
        &self,
        _filter: ProspectFilter,
    ) -> Result<Vec<crate::queries::ProspectionPipelineRow>, DomainError> {
        Ok(self.rows.lock().unwrap().clone())
    }
}

// ------------------------------------------------------------------------------------------------
// Service doubles
// ------------------------------------------------------------------------------------------------

/// Stripe gateway double: canonical intent `pi_123`/`pi_123_secret`; declines exactly the
/// `pm_declined` payment method (the spec's rejection sample).
#[derive(Default)]
pub struct FakeGateway;

#[async_trait]
impl PaymentService for FakeGateway {
    async fn request(
        &self,
        input: PaymentRequestInput,
        _meta: &ServiceCallMeta,
    ) -> Result<PaymentRequestOutput, DomainError> {
        if input.payment_method_id.0 == "pm_declined" {
            return Err(DomainError::rejected(
                "PaymentDeclined",
                serde_json::json!({ "reason": "card_declined" }),
            ));
        }
        Ok(PaymentRequestOutput {
            payment_intent_id: PaymentIntentId("pi_123".into()),
            client_secret: "pi_123_secret".into(),
        })
    }
    async fn refund(&self, _input: PaymentRefundInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeDelivery;

#[async_trait]
impl DeliveryService for FakeDelivery {
    async fn offer_job(&self, _input: DeliveryOfferJobInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        Ok(())
    }
}

/// Identity double: any phone verifies to the spec's `auth-supabase-1`; the canonical bad code
/// `000000` (and bad email token `bad-token`) is rejected like the Supabase ACL would.
#[derive(Default)]
pub struct FakeIdentity;

#[async_trait]
impl IdentityService for FakeIdentity {
    async fn send_phone_otp(&self, _input: IdentitySendPhoneOtpInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        Ok(())
    }
    async fn verify_phone_otp(
        &self,
        input: IdentityVerifyPhoneOtpInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityVerifyPhoneOtpOutput, DomainError> {
        if input.code.0 == "000000" {
            return Err(DomainError::rejected("InvalidVerificationCode", serde_json::json!({})));
        }
        Ok(IdentityVerifyPhoneOtpOutput { auth_ref: ExternalReference("auth-supabase-1".into()) })
    }
    async fn send_email_magic_link(
        &self,
        _input: IdentitySendEmailMagicLinkInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        Ok(())
    }
    async fn verify_email_token(
        &self,
        input: IdentityVerifyEmailTokenInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityVerifyEmailTokenOutput, DomainError> {
        if input.token.0 == "bad-token" {
            return Err(DomainError::rejected("InvalidVerificationToken", serde_json::json!({})));
        }
        Ok(IdentityVerifyEmailTokenOutput {
            auth_ref: ExternalReference("auth-supabase-1".into()),
            email: EmailAddress("johnny@example.com".into()),
        })
    }
}

/// GBP ownership double: any proof verifies except the spec's canonical bad one (`bad-token`).
#[derive(Default)]
pub struct FakeOwnership;

#[async_trait]
impl GoogleOwnershipVerifier for FakeOwnership {
    async fn verify(&self, _restaurant_id: RestaurantId, proof: &str) -> Result<bool, DomainError> {
        Ok(!proof.contains("bad"))
    }
}

/// GBP order-link probe double: always observes VERIFIED (the spec's sample outcome).
#[derive(Default)]
pub struct FakeProbe;

#[async_trait]
impl GbpOrderLinkProbe for FakeProbe {
    async fn probe(&self, _url: &WebUrl) -> Result<GbpLinkStatus, DomainError> {
        Ok(GbpLinkStatus::VERIFIED)
    }
}

/// Money helper for canned rows.
pub fn eur(cents: i64) -> Money {
    Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
}
