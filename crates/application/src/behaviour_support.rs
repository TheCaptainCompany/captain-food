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
    IdentityStampRiderClaimInput,
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
    CartReadRepository, CatalogReadRepository, CustomerReadRepository, MemberIdentityRepository,
    OfferView, OrderFilter, OrderReadRepository, PlatformMemberRepository, ProspectFilter,
    ProspectionReadRepository, RestaurantFilter, RestaurantReadRepository, RiderIdentityRepository,
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
    // #639 part C step 6-iv round 2 (ADR-20260905-101349 §2 amendment): the invitation-based
    // grant's membershipId is UUIDv5-DERIVED from invitationId, never a free label -- so the
    // fixture pool computes the SAME derivation a `then:` fixture asserts (the `deliv-1`
    // precedent below), for ANY `membership-from-invitation-{label}`, generically.
    if let Some(label) = s.strip_prefix("membership-from-invitation-") {
        return crate::commands::restaurant_membership_id_for_invitation(
            domain::generated::scalars::RestaurantInvitationId(uid(&format!("invitation-{label}"))),
        )
        .0;
    }
    // Round 2, R2-5 (#639 part C step 6-v): `platformMembershipId` must equal
    // `platform_membership_id_for(authSubject)` -- the SAME formula the handler now enforces --
    // so the fixture pool computes the SAME derivation for ANY `platform-membership-for-{subject}`,
    // generically, the `membership-from-invitation-` precedent above.
    if let Some(subject) = s.strip_prefix("platform-membership-for-") {
        return crate::commands::platform_membership_id_for(subject).0;
    }
    match s {
        "deliv-1" => {
            crate::process_managers::delivery_dispatch::delivery_job_id_for(&OrderId(uid("order-1"))).0
        }
        _ => uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, s.as_bytes()),
    }
}

/// The fixed command-side actor (ADMIN-level envelope identity; the envelope is not asserted).
pub fn actor() -> Actor {
    actor_as("ADMIN")
}

/// A command-side actor with an explicit UserType AND resolved domain identity — what
/// `when.principal` in tests.yaml dispatches as, exercising the `requires` acting/claims checks
/// (#235, PROP-20260728-135632 §2.2).
pub fn actor_principal(user_type: &str, domain_id: Option<uuid::Uuid>) -> Actor {
    Actor { domain_id, ..actor_as(user_type) }
}

/// A command-side actor with an explicit UserType TEXT value — for the handlers whose semantics
/// derive from the acting persona (e.g. `TipOrder`'s `tippedBy`, ADR-0041).
pub fn actor_as(user_type: &str) -> Actor {
    Actor {
        user_id: uuid::Uuid::from_u128(0xA0),
        user_type: user_type.to_string(),
        domain_id: None,
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
    pub slugs: SpecSlugReservations,
    pub auth_subjects: SpecAuthSubjectReservations,
    pub mailbox_requeue: SpecMailboxRequeue,
    pub catalogs: SpecCatalogs,
    pub carts: SpecCarts,
    pub customers: SpecCustomers,
    pub orders: SpecOrders,
    pub prospection: SpecProspection,
    pub payments: FakeGateway,
    pub delivery: FakeDelivery,
    pub dispatch_config: SpecDispatchConfig,
    pub identity: FakeIdentity,
    pub ownership: FakeOwnership,
    pub probe: FakeProbe,
    /// Cookie-pickup parking (#112) — VerifyPhone parks the provider session here.
    pub auth_sessions: crate::auth_sessions::mem::MemAuthSessionStore,
    /// The `Rider` read model's identity bridge (`auth_ref -> rider_id`, #639 part C step 2b) the
    /// rider sign-in door identifies through — fed by seeded `RiderRegistered` facts.
    pub riders: SpecRiders,
    /// The `Member` read model's identity bridge (`auth_subject -> member_id`, #639 part C step
    /// 6-ii) the member sign-in door identifies through — fed by seeded `RestaurantAccessGranted`
    /// facts.
    pub members: SpecMembers,
    /// The `PlatformMember` bridge (`auth_subject -> platformMembershipId`, #639 part C step 6-v,
    /// ADR-20260905-223957 §1) `grant_platform_access`'s "two arbiters" check consults -- fed by
    /// seeded `PlatformAccessGranted` facts, sentinel-seeded like `SpecAuthSubjectReservations`.
    pub platform_members: SpecPlatformMembers,
    /// `SUPPORT_CONTACT` as the composition root resolves it (required, no default —
    /// ADR-20260830-213135); the bed carries the decided string so the refusal can name it.
    pub support_contact: SpecSupportContact,
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
            // `*Updated` carries the FULL entity (replace semantics), and the real `Catalog`
            // projection is fed by it (projection_tables.yaml#/Catalog `fedBy`), rebuilding the
            // offer tree — so an offer's availability/price move here exactly as they do in
            // production. This is how a kitchen 86's a dish: the offer stays in the catalog and
            // only its availability flag flips.
            DomainEvent::ProductUpdated(e) => {
                for offer in &e.product.offers {
                    self.catalogs.add_offer(e.restaurant_id, offer_view(&e.product.name.0, offer));
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
            // --- Rider identity bridge (#639 part C step 2b) ------------------------------------
            DomainEvent::RiderRegistered(e) => {
                self.riders.bind(&e.auth_ref.0, e.rider_id);
            }
            // --- Member identity bridge (#639 part C step 6-ii) -----------------------------
            DomainEvent::RestaurantAccessGranted(e) => {
                self.members.bind(&e.auth_subject.0, e.member_id);
            }
            // --- Platform member bridge (#639 part C step 6-v, ADR-20260905-223957 §1) -----
            DomainEvent::PlatformAccessGranted(e) => {
                self.platform_members.bind(&e.auth_subject.0, e.platform_membership_id);
            }
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
                        customer_id: Some(e.checkout.customer_id),
                        session_id: None,
                        client_secret: Some("pi_123_secret".into()),
                        last_processed_stripe_event_id: None,
                        last_update_utc: chrono::Utc::now(),
                    })
                    .await
                    .expect("seed payment run");
            }
            DomainEvent::PaymentAuthorized(e) => {
                if let Some(order_id) = e.order_id {
                    self.orders.set_payment(order_id, "AUTHORIZED", &e.payment_intent_id);
                }
                self.orders.set_payment_by_intent("AUTHORIZED", &e.payment_intent_id);
            }
            DomainEvent::PaymentCaptured(e) => {
                if let Some(order_id) = e.order_id {
                    self.orders.set_payment(order_id, "CAPTURED", &e.payment_intent_id);
                }
                self.orders.set_payment_by_intent("CAPTURED", &e.payment_intent_id);
            }
            DomainEvent::PaymentReleased(e) => {
                if let Some(order_id) = e.order_id {
                    self.orders.set_payment(order_id, "RELEASED", &e.payment_intent_id);
                }
                self.orders.set_payment_by_intent("RELEASED", &e.payment_intent_id);
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
                        current_rank: Some(1),
                        current_channel: Some(DeliveryChannelKey("independent".into())),
                        last_update_utc: chrono::Utc::now(),
                    })
                    .await
                    .expect("seed dispatch run");
            }
            DomainEvent::DeliveryRejectedByPartner(e) => {
                // Mirror the saga's ranked-walk advance (#60): a seeded decline advances the run to
                // the next ranked channel (offer_attempts += 1, current_rank/current_channel step).
                if let Some(row) = self
                    .dispatch_pm
                    .by_delivery_job(e.delivery_job_id)
                    .await
                    .expect("dispatch run lookup")
                {
                    let next_rank = row.current_rank.unwrap_or(0) + 1;
                    self.dispatch_pm
                        .upsert(&DeliveryDispatchRow {
                            offer_attempts: row.offer_attempts + 1,
                            current_rank: Some(next_rank),
                            current_channel: harness_channel_at(next_rank),
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
            .unwrap_or_else(|| PaymentIntentId("pi_123".into()));
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
        slug: None,   // no storefront address until the owner configures one
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
        customer_id: Some(e.customer_id),
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
        payment_intent_id: e.payment_intent_id.clone(),
        // Authorize-then-capture (ADR-20260808-195315 §1.2): OrderPlaced happens on AUTHORIZATION,
        // so a charging order seeds AUTHORIZED (capture follows on fulfilment); a $0 replacement
        // (no intent) keeps the historical CAPTURED = "nothing owed" — mirrors the projector
        // (crate::projectors::order_tracking payment_status hook).
        payment_status: if e.payment_intent_id.is_some() { "AUTHORIZED" } else { "CAPTURED" }
            .to_string(),
        restaurant_stars: None,
        rating_comment: None,
        rider_thumb: None,
        delivery_timeliness: None,
        rider_tip_cents: None,
        restaurant_tip_cents: None,
        captain_tip_cents: None,
        rated_at: None,
        delivery_status: None,
        courier: None,
        estimated_dropoff_at: None,
        delivery_handed_back: false,
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

/// In-memory `SlugReservationRepository` (ADR-20260728-011344 D3).
///
/// Seeded with `already-held`, owned by a foreign restaurant: that is tests.yaml's "held by someone
/// else" fixture, and it is what makes `TestStorefrontSlugTakenIsRejected` a real assertion rather
/// than a tautology. Reservations are otherwise driven by the handler under test.
pub struct SpecSlugReservations {
    /// slug -> the restaurant holding it. A released label KEEPS its entry (never reusable).
    held: std::sync::Mutex<std::collections::HashMap<String, RestaurantId>>,
}

impl Default for SpecSlugReservations {
    fn default() -> Self {
        let mut held = std::collections::HashMap::new();
        held.insert("already-held".to_string(), RestaurantId(uuid::Uuid::from_u128(0xA11EAD)));
        Self { held: std::sync::Mutex::new(held) }
    }
}

/// In-memory `AuthSubjectReservationRepository` (#639 part C step 2a, #794), sentinel-seeded like
/// [`SpecSlugReservations`]: `(RIDER, "already-bound")` is held by a FOREIGN rider id, which is what
/// makes `TestRiderAuthSubjectAlreadyBoundIsRejected` a real assertion -- rider-2 has no aggregate,
/// so without the seeded holder the fold alone would accept. There is no release method to fake:
/// the port has none.
pub struct SpecAuthSubjectReservations {
    /// (principal_kind, subject) -> the principal id holding it. Never removed.
    held: std::sync::Mutex<std::collections::HashMap<(PrincipalKind, String), uuid::Uuid>>,
}

impl Default for SpecAuthSubjectReservations {
    fn default() -> Self {
        let mut held = std::collections::HashMap::new();
        held.insert(
            (PrincipalKind::RIDER, "already-bound".to_string()),
            uuid::Uuid::from_u128(0xB0B),
        );
        // #639 part C step 6-i (ADR-20260905-101349 §4): the SAME shape, for MEMBER --
        // `TestGrantRestaurantAccessAuthSubjectAlreadyBoundIsRejected` names a DIFFERENT memberId
        // than the one holding this row, so the fold alone would accept without this seed.
        held.insert(
            (PrincipalKind::MEMBER, "already-bound-member".to_string()),
            uuid::Uuid::from_u128(0xB0B5757),
        );
        // #639 part C step 6-iv round 2 (young B1): "auth-rehire" already holds an EXISTING
        // memberId -- `TestGrantRestaurantAccessByInvitationReusesHeldMemberId` invites the SAME
        // person again with a FRESH, DIFFERENT minted memberId and asserts the grant lands the
        // HELD one, never the freshly-minted one.
        held.insert((PrincipalKind::MEMBER, "auth-rehire".to_string()), uid("member-existing-rehire"));
        Self { held: std::sync::Mutex::new(held) }
    }
}

#[async_trait]
impl crate::queries::AuthSubjectReservationRepository for SpecAuthSubjectReservations {
    async fn reserve(
        &self,
        subject: AuthSubject,
        principal: crate::queries::BoundPrincipal,
    ) -> Result<bool, DomainError> {
        let mut held = self.held.lock().unwrap();
        match held.get(&(principal.kind(), subject.0.clone())) {
            // Already ours: an idempotent replay, not a conflict.
            Some(holder) if *holder == principal.id() => Ok(true),
            Some(_) => Ok(false),
            None => {
                held.insert((principal.kind(), subject.0), principal.id());
                Ok(true)
            }
        }
    }

    async fn holder_of(
        &self,
        subject: AuthSubject,
        kind: PrincipalKind,
    ) -> Result<Option<uuid::Uuid>, DomainError> {
        let held = self.held.lock().unwrap();
        Ok(held.get(&(kind, subject.0)).copied())
    }
}

/// In-memory `MailboxRequeue` (#315), sentinel-seeded like [`SpecSlugReservations`]:
/// `uid("poisoned-1")` is a cap-poisoned Payment-lane row (the happy path flips it and returns
/// the lane), `uid("settled-1")` exists but SUCCEEDED (NotRequeueable), anything else is unknown
/// to the mailbox (NotFound). The sentinels are what make the tests.yaml cases real assertions
/// rather than tautologies — port state is not declarable in YAML, so it lives here by
/// convention and the spec references the literals.
pub struct SpecMailboxRequeue {
    /// message_id -> (poisoned?, actor_type-or-status). Flipped entries move to deliverable.
    rows: std::sync::Mutex<std::collections::HashMap<uuid::Uuid, SpecMailboxRow>>,
}

enum SpecMailboxRow {
    Poisoned { actor_type: String },
    Deliverable { actor_type: String },
    Terminal { status: String },
}

impl Default for SpecMailboxRequeue {
    fn default() -> Self {
        let mut rows = std::collections::HashMap::new();
        rows.insert(uid("poisoned-1"), SpecMailboxRow::Poisoned { actor_type: "Payment".into() });
        rows.insert(uid("settled-1"), SpecMailboxRow::Terminal { status: "SUCCEEDED".into() });
        Self { rows: std::sync::Mutex::new(rows) }
    }
}

#[async_trait]
impl crate::queries::MailboxRequeue for SpecMailboxRequeue {
    async fn requeue_if_poisoned(
        &self,
        message_id: uuid::Uuid,
        _access: crate::queries::MailboxRequeueAccess,
    ) -> Result<crate::queries::RequeueOutcome, DomainError> {
        use crate::queries::RequeueOutcome;
        let mut rows = self.rows.lock().unwrap();
        match rows.get(&message_id) {
            Some(SpecMailboxRow::Poisoned { actor_type }) => {
                let actor_type = actor_type.clone();
                rows.insert(
                    message_id,
                    SpecMailboxRow::Deliverable { actor_type: actor_type.clone() },
                );
                Ok(RequeueOutcome::Requeued { actor_type })
            }
            Some(SpecMailboxRow::Deliverable { actor_type }) => {
                Ok(RequeueOutcome::AlreadyDeliverable { actor_type: actor_type.clone() })
            }
            Some(SpecMailboxRow::Terminal { status }) => {
                Ok(RequeueOutcome::NotRequeueable { status: status.clone() })
            }
            None => Ok(RequeueOutcome::NotFound),
        }
    }
}

#[async_trait]
impl crate::queries::SlugReservationRepository for SpecSlugReservations {
    async fn reserve(&self, slug: Slug, restaurant_id: RestaurantId) -> Result<bool, DomainError> {
        let mut held = self.held.lock().unwrap();
        match held.get(&slug.0) {
            // Already ours: an idempotent replay, not a conflict.
            Some(owner) if *owner == restaurant_id => Ok(true),
            Some(_) => Ok(false),
            None => {
                held.insert(slug.0, restaurant_id);
                Ok(true)
            }
        }
    }
    async fn release(&self, _slug: Slug, _restaurant_id: RestaurantId) -> Result<(), DomainError> {
        // Deliberately a no-op on the entry: a released label stays reserved so its redirect cannot
        // be hijacked. Only the "is this my current address" question moves, and that lives in the
        // event log, not here.
        Ok(())
    }
}

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
        Ok(self.rows.lock().unwrap().iter().find(|r| r.slug.as_ref() == Some(&slug)).cloned())
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
    /// Sentinel (same shape as the other TestBed fakes): a label containing `taken` is reported as
    /// already used by a SIBLING catalog of the same restaurant. `by_restaurant` returns `None` here,
    /// so the default derivation could never reach CatalogSlugAlreadyTaken -- the per-restaurant
    /// uniqueness is a read-model fact, and this is the read model in a spec test.
    async fn slug_taken(
        &self,
        _restaurant_id: RestaurantId,
        slug: &Slug,
        _excluding: CatalogId,
    ) -> Result<bool, DomainError> {
        Ok(slug.0.contains("taken"))
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
    /// Tenant-scoped leg 1 of `cart.current` (#469). The restaurant predicate is part of the PORT
    /// contract, so this double honours it: a double that ignored it would let a tenant-blind
    /// implementation pass its tests.
    async fn open_by_customer_at(
        &self,
        customer_id: CustomerId,
        restaurant_id: RestaurantId,
    ) -> Result<Vec<crate::queries::CartRow>, DomainError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|r| {
                r.customer_id == Some(customer_id)
                    && r.restaurant_id == restaurant_id
                    && r.status == CartStatus::OPEN
            })
            .cloned()
            .collect())
    }
    /// Tenant-scoped leg 2 of `cart.current` (#469), same obligation.
    async fn open_by_session_at(
        &self,
        session_id: SessionId,
        restaurant_id: RestaurantId,
    ) -> Result<Vec<crate::queries::CartRow>, DomainError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|r| {
                r.session_id == session_id
                    && r.restaurant_id == restaurant_id
                    && r.status == CartStatus::OPEN
            })
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
        // The stored column is an `AuthSubject` (#639 part C step 1) while this PORT still takes
        // an `ExternalReference`: its two remaining callers mint one in code this step does not
        // own -- the emitted `me` resolver (server_graphql.rs) and the mailbox handler. Comparing
        // the strings keeps the fake honest and keeps the mismatch VISIBLE; it disappears when the
        // port is retyped (step 1b).
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|r| r.auth_ref.as_ref().map(|a| &a.0) == Some(&auth_ref.0))
            .cloned())
    }
}

/// The `Rider` read model's identity bridge as a fake (#639 part C step 2c-i): `auth_ref ->
/// rider_id`, one column out, bound by seeded `RiderRegistered` facts. Answers WHO this login is
/// and nothing else -- the read model's own rule (an identity index is not an authorization oracle).
#[derive(Default)]
pub struct SpecRiders {
    rows: Mutex<Vec<(String, RiderId)>>,
}

impl SpecRiders {
    fn bind(&self, auth_ref: &str, rider_id: RiderId) {
        let mut rows = self.rows.lock().unwrap();
        rows.retain(|(a, _)| a != auth_ref);
        rows.push((auth_ref.to_string(), rider_id));
    }
}

/// The `Member` read model's identity bridge as a fake (#639 part C step 6-ii): `auth_subject ->
/// member_id`, bound by seeded `RestaurantAccessGranted` facts. The `SpecRiders` precedent.
#[derive(Default)]
pub struct SpecMembers {
    rows: Mutex<Vec<(String, MemberId)>>,
}

impl SpecMembers {
    fn bind(&self, auth_subject: &str, member_id: MemberId) {
        let mut rows = self.rows.lock().unwrap();
        rows.retain(|(a, _)| a != auth_subject);
        rows.push((auth_subject.to_string(), member_id));
    }
}

/// The `PlatformMember` bridge as a fake (#639 part C step 6-v, ADR-20260905-223957 §1):
/// `auth_subject -> platformMembershipId`, bound by seeded `PlatformAccessGranted` facts. The
/// `SpecAuthSubjectReservations` sentinel-seeding precedent: `"already-granted-admin"` is held by
/// a FOREIGN `platformMembershipId` (`uid("platform-membership-existing")`, never
/// `platform_membership_id_for("already-granted-admin")`, the CORRECTLY-DERIVED id the colliding
/// test dispatches post round-2 R2-5), which is what makes
/// `TestGrantPlatformAccessAuthSubjectAlreadyGrantedIsRejected` a real assertion reaching the
/// bridge check at all (the id-derivation check now runs FIRST) -- without the seeded holder,
/// `grant_platform_access`'s bridge lookup alone would find nothing and accept.
pub struct SpecPlatformMembers {
    rows: Mutex<Vec<(String, PlatformMembershipId)>>,
}

impl Default for SpecPlatformMembers {
    fn default() -> Self {
        Self {
            rows: Mutex::new(vec![(
                "already-granted-admin".to_string(),
                PlatformMembershipId(uid("platform-membership-existing")),
            )]),
        }
    }
}

impl SpecPlatformMembers {
    fn bind(&self, auth_subject: &str, platform_membership_id: PlatformMembershipId) {
        let mut rows = self.rows.lock().unwrap();
        rows.retain(|(a, _)| a != auth_subject);
        rows.push((auth_subject.to_string(), platform_membership_id));
    }
}

#[async_trait]
impl PlatformMemberRepository for SpecPlatformMembers {
    async fn platform_membership_id_by_auth_subject(
        &self,
        auth_subject: AuthSubject,
    ) -> Result<Option<PlatformMembershipId>, DomainError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|(a, _)| *a == auth_subject.0)
            .map(|(_, id)| *id))
    }
}

#[async_trait]
impl MemberIdentityRepository for SpecMembers {
    async fn member_id_by_auth_subject(
        &self,
        auth_subject: AuthSubject,
    ) -> Result<Option<MemberId>, DomainError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|(a, _)| *a == auth_subject.0)
            .map(|(_, id)| *id))
    }
}

#[async_trait]
impl RiderIdentityRepository for SpecRiders {
    async fn rider_id_by_auth_subject(
        &self,
        auth_subject: AuthSubject,
    ) -> Result<Option<(RiderId, domain::generated::scalars::RiderStanding)>, DomainError> {
        // Behaviour tests never script a restricted-rider seam (that lives on the server-side
        // guard tests) — every bound rider here is ACTIVE.
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|(a, _)| *a == auth_subject.0)
            .map(|(_, id)| (*id, domain::generated::scalars::RiderStanding::ACTIVE)))
    }
}

/// `SUPPORT_CONTACT` as the bed resolves it: the decided string (SUPPORT-CONTACT, 2026-08-31), so
/// the rider sign-in refusal names a route. `None` is the composition root's dev-only unset case,
/// which the handler refuses loudly -- not a case a behaviour test drives.
pub struct SpecSupportContact(pub Option<EmailAddress>);

impl Default for SpecSupportContact {
    fn default() -> Self {
        Self(Some(EmailAddress("support@captain.food".into())))
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
    async fn list(&self, filter: OrderFilter, _scope: &crate::queries::ReadScope) -> Result<Vec<crate::queries::OrderTrackingRow>, DomainError> {
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
    async fn by_id(&self, id: OrderId, _scope: &crate::queries::ReadScope) -> Result<Option<crate::queries::OrderTrackingRow>, DomainError> {
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
    async fn capture(
        &self,
        _input: crate::generated::services::PaymentCaptureInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        Ok(())
    }
    async fn release(
        &self,
        _input: crate::generated::services::PaymentReleaseInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        Ok(())
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

/// The harness's fixed ranked walk (#60) — 3 channels so the spec's exhaustion fixture (two prior
/// declines, then a third that fails closed with `attempts: 3`) is reachable. Shared by
/// [`SpecDispatchConfig`] (the saga's live resolution) and the seed-mirror in `apply_effects` (which
/// steps a seeded decline's `current_rank`) so both stay consistent.
fn harness_channel_at(rank: i32) -> Option<DeliveryChannelKey> {
    match rank {
        1 => Some(DeliveryChannelKey("independent".into())),
        2 => Some(DeliveryChannelKey("uber_direct".into())),
        3 => Some(DeliveryChannelKey("coopcycle".into())),
        _ => None,
    }
}

/// Dispatch-strategy double (#60): every restaurant is CAPTAIN-dispatched with the harness ranking —
/// the birth leg offers rank-1 and opens OFFERED.
#[derive(Default)]
pub struct SpecDispatchConfig;

#[async_trait]
impl crate::dispatch_strategy::DispatchStrategyRepository for SpecDispatchConfig {
    async fn restaurant_dispatch(
        &self,
        _restaurant_id: RestaurantId,
    ) -> Result<crate::dispatch_strategy::RestaurantDispatch, DomainError> {
        Ok(crate::dispatch_strategy::RestaurantDispatch {
            mode: RestaurantDispatchMode::CAPTAIN,
            city_id: None,
        })
    }
    async fn ranked_channels(
        &self,
        _city_id: Option<CityId>,
    ) -> Result<Vec<crate::dispatch_strategy::RankedChannel>, DomainError> {
        Ok((1..=3)
            .map(|rank| crate::dispatch_strategy::RankedChannel {
                rank,
                channel: harness_channel_at(rank).unwrap(),
                ttl_override_seconds: None,
            })
            .collect())
    }
    async fn channel_default_ttl_seconds(
        &self,
        _channel: &DeliveryChannelKey,
    ) -> Result<Option<i32>, DomainError> {
        Ok(Some(120))
    }
}

/// Identity double: any phone verifies to the spec's `auth-supabase-1`; the canonical bad code
/// `000000` (and bad email token `bad-token`) is rejected like the Supabase ACL would.
///
/// STATEFUL for the #437 claim-stamp flow: `stamp_customer_claim` records the `app_metadata` a
/// real provider would then hold, and `refresh_session` mints an unsigned but DECODABLE JWT
/// (`hdr.<base64url(payload)>.sig`) whose payload reflects ONLY what was stamped — no stamp
/// recorded means no claim in the token. Tests therefore pin the stamp→rotate→park ORDERING by
/// decoding the parked token (behaviour), never by asserting a call log.
///
/// **`sent` is the one call log this file allows, and #516 is why.** The convention here is that a
/// call log tests structure rather than behaviour — but for an SMS OTP the outbound call IS the
/// behaviour, because it is the money. `assert_appended(&before, &[])` passes whether or not an SMS
/// went out (the command emits no event), so before this field existed "no SMS was attempted" was
/// literally unassertable and every send guard would have been unproven. Assert
/// `bed.identity.sent().is_empty()`, never just the response.
///
/// It also enforces the REAL [`SmsSendPolicy`] over an in-memory counter, so a behaviour test drives
/// the same allowlist decision the served adapters make instead of a lookalike.
#[derive(Default)]
pub struct FakeIdentity {
    /// The `app_metadata` the provider holds after a stamp (`None` = never stamped).
    stamped: Mutex<Option<serde_json::Value>>,
    /// Every phone-OTP send this double was asked to make AND authorised. A refused send records
    /// nothing — that absence is the assertion.
    sent: Mutex<Vec<IdentitySendPhoneOtpInput>>,
    /// Per-process counters for the send guards. In-memory ON PURPOSE here (see
    /// [`InMemorySmsQuotaStore`]); the served path uses the shared Postgres store.
    quota: crate::sms_guard::InMemorySmsQuotaStore,
}

/// The login the fake provider resolves a verified phone to -- a provider maps phone -> user, and
/// the rider door (#639 part C step 2c-i) needs THREE distinguishable logins: the riderRegistered
/// fixture's (`+33611223344` -> `auth-supabase-9`), a login the provider already holds STAMPED as a
/// CUSTOMER (the one-subject-one-role sentinel, `+33699000002` -> `auth-supabase-customer`), and
/// everybody else (`auth-supabase-1`, the customer suite's canonical subject -- unchanged).
fn fake_auth_subject_for(national_number: &str) -> AuthSubject {
    AuthSubject(
        match national_number.trim_start_matches('0') {
            "611223344" => "auth-supabase-9",
            "699000002" => FAKE_CUSTOMER_STAMPED_SUBJECT,
            _ => "auth-supabase-1",
        }
        .into(),
    )
}

/// A login the fake provider holds ALREADY STAMPED with a customer claim (see
/// [`fake_auth_subject_for`]): stamping RIDER on it would erase that claim, so the rider stamp
/// refuses it -- the sentinel that drives `AuthSubjectHoldsAnotherRole`.
const FAKE_CUSTOMER_STAMPED_SUBJECT: &str = "auth-supabase-customer";

impl FakeIdentity {
    /// The phone-OTP sends this double actually made. **The money assertion**: a guarded refusal must
    /// leave this empty, and a rejection alone does not prove that nothing was sent.
    pub fn sent(&self) -> Vec<IdentitySendPhoneOtpInput> {
        self.sent.lock().expect("FakeIdentity poisoned").clone()
    }

    /// The `app_metadata` the provider holds after the last stamp (`None` = never stamped) --
    /// what a rotated token would carry.
    pub fn stamped(&self) -> Option<serde_json::Value> {
        self.stamped.lock().expect("fake identity mutex").clone()
    }
}

#[async_trait]
impl IdentityService for FakeIdentity {
    async fn send_phone_otp(&self, input: IdentitySendPhoneOtpInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        // The guards run HERE because the served adapters run them here too (the ACL boundary and
        // the send seam): a behaviour test then exercises the real policy, not a double of it.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        crate::sms_guard::SmsSendPolicy::default()
            .authorize(&self.quota, &input.dialing_code, &input.national_number, now)
            .await
            .map_err(|refusal| refusal.into_domain_error())?;
        self.sent.lock().expect("FakeIdentity poisoned").push(input);
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
        Ok(IdentityVerifyPhoneOtpOutput {
            auth_ref: fake_auth_subject_for(&input.national_number.0),
            // The provider session (#112) — a fake token trio; the handler parks it.
            access_token: Some("fake.access.jwt".into()),
            refresh_token: Some("fake.refresh".into()),
            expires_in: Some(3600),
        })
    }
    async fn refresh_session(
        &self,
        _input: crate::generated::services::IdentityRefreshSessionInput,
        _meta: &ServiceCallMeta,
    ) -> Result<crate::generated::services::IdentityRefreshSessionOutput, DomainError> {
        // The rotated token reflects ONLY what was stamped so far (a real provider re-mints the
        // JWT from the user's CURRENT app_metadata at rotation): unsigned but decodable, so a
        // test can read the claims out of whatever the handler parked.
        use base64::Engine as _;
        let app_metadata =
            self.stamped.lock().expect("fake identity mutex").clone().unwrap_or_else(|| serde_json::json!({}));
        let payload = serde_json::json!({ "app_metadata": app_metadata });
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&payload).expect("fake JWT payload serializes"));
        Ok(crate::generated::services::IdentityRefreshSessionOutput {
            access_token: format!("hdr.{b64}.sig"),
            refresh_token: Some("fake.refresh.2".into()),
            expires_in: Some(3600),
        })
    }
    async fn stamp_customer_claim(
        &self,
        input: crate::generated::services::IdentityStampCustomerClaimInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        // Record what a correct provider would hold after the stamp: BOTH claims, inside the ONE
        // product-owned object, per the shallow-merge rule of services.yaml
        // identity.stamp_customer_claim (nested by #519 -- the verifier refuses a token without it).
        *self.stamped.lock().expect("fake identity mutex") = Some(serde_json::json!({
            "captain_food": {
                "role": "CUSTOMER",
                "customer_id": input.customer_id.0.to_string(),
            }
        }));
        Ok(())
    }
    async fn stamp_rider_claim(
        &self,
        input: IdentityStampRiderClaimInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        // The one-subject-one-role refusal (PROP-20260831-180622 Concern): a login the provider
        // already holds with ANOTHER claim object is refused, never overwritten -- the sentinel
        // subject is such a login, and so is a subject this bed stamped CUSTOMER earlier.
        let mut stamped = self.stamped.lock().expect("fake identity mutex");
        let rider_only = serde_json::json!({ "captain_food": { "role": "RIDER" } });
        let holds_other = input.auth_ref.0 == FAKE_CUSTOMER_STAMPED_SUBJECT
            || stamped.as_ref().is_some_and(|held| *held != rider_only);
        if holds_other {
            return Err(DomainError::rejected(
                "AuthSubjectHoldsAnotherRole",
                serde_json::json!({ "authRef": input.auth_ref.0 }),
            ));
        }
        // The provider holds `{ role: RIDER }` and NOTHING else (services.yaml
        // identity.stamp_rider_claim) -- a rotated token then carries exactly that.
        *stamped = Some(rider_only);
        Ok(())
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
        // #639 part C step 6-ii: the ONE-SUBJECT-ONE-ROLE collision sentinel token, proving the
        // SAME auth subject `stamp_rider_claim`'s fixture already treats as CUSTOMER-stamped
        // (`FAKE_CUSTOMER_STAMPED_SUBJECT`) — the member door's `riderRegisteredOnCustomerLogin`
        // precedent.
        // #639 part C step 6-iv round 2: a SECOND named subject, proving "auth-rehire" -- the
        // `SpecAuthSubjectReservations` seed's re-hire sentinel (see its `Default` impl).
        let auth_ref = if input.token.0 == "sb-magic-token-customer-login" {
            AuthSubject(FAKE_CUSTOMER_STAMPED_SUBJECT.into())
        } else if input.token.0 == "sb-magic-token-rehire" {
            AuthSubject("auth-rehire".into())
        } else {
            AuthSubject("auth-supabase-1".into())
        };
        Ok(IdentityVerifyEmailTokenOutput {
            auth_ref,
            email: EmailAddress("johnny@example.com".into()),
            access_token: Some("fake.access.jwt".into()),
            refresh_token: Some("fake.refresh".into()),
            expires_in: Some(3600),
        })
    }
    async fn stamp_member_claim(
        &self,
        input: crate::generated::services::IdentityStampMemberClaimInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        // The one-subject-one-role refusal, the `stamp_rider_claim` precedent: a login the
        // provider already holds with ANOTHER claim object is refused, never overwritten.
        let mut stamped = self.stamped.lock().expect("fake identity mutex");
        let member_only = serde_json::json!({ "captain_food": { "role": "MEMBER" } });
        let holds_other = input.auth_ref.0 == FAKE_CUSTOMER_STAMPED_SUBJECT
            || stamped.as_ref().is_some_and(|held| *held != member_only);
        if holds_other {
            return Err(DomainError::rejected(
                "AuthSubjectHoldsAnotherRole",
                serde_json::json!({ "authRef": input.auth_ref.0 }),
            ));
        }
        *stamped = Some(member_only);
        Ok(())
    }
    async fn stamp_admin_claim(
        &self,
        input: crate::generated::services::IdentityStampAdminClaimInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        // The one-subject-one-role refusal, the `stamp_member_claim` precedent: a login the
        // provider already holds with ANOTHER claim object is refused, never overwritten.
        let mut stamped = self.stamped.lock().expect("fake identity mutex");
        let admin_only = serde_json::json!({ "captain_food": { "role": "ADMIN" } });
        let holds_other = input.auth_ref.0 == FAKE_CUSTOMER_STAMPED_SUBJECT
            || stamped.as_ref().is_some_and(|held| *held != admin_only);
        if holds_other {
            return Err(DomainError::rejected(
                "AuthSubjectHoldsAnotherRole",
                serde_json::json!({ "authRef": input.auth_ref.0 }),
            ));
        }
        *stamped = Some(admin_only);
        Ok(())
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
