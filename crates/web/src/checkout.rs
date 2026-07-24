//! The checkout flow + screen (split 3/4 of #21) — `restaurant_frontoffice.yaml#/screens/checkout`,
//! deliberately NOT SDUI (`sdui: false`: "Stripe Elements + payment security").
//!
//! The flow is acceptance-first end to end (ADR-20260720-015500):
//!
//!   1. The client MINTS the `orderId` (UUIDv7 — `PlaceOrder.orderId` is command input by spec:
//!      "Client-generated id for the order the saga will materialize on payment capture"), so the
//!      confirmation route `/orders/{orderId}/confirmation` is known BEFORE the server answers.
//!   2. [`submit`] dispatches `place_order` through the two-step dispatcher (`actions::dispatch`) —
//!      the mutation returns only the acceptance envelope.
//!   3. The payment outcome is a READ: [`PlacedOrder::await_payment_intent`] polls
//!      `paymentStatus.byOrder` until the PlaceOrderProcess has created the Stripe intent and its
//!      `clientSecret` exists — the `paymentStatusChanged` subscription (`subscriptions.rs`) is the
//!      push accelerator for the same data on the hydrate path.
//!   4. The Stripe element (`stripe.rs`) confirms against that clientSecret; capture lands as an
//!      inbound Stripe fact server-side, never as a client claim.
//!
//! Losing the tab between 2 and 4 loses nothing: the session id survives restarts (#12), the
//! acceptance is journaled under the minted messageId, and re-entering the confirmation route
//! re-resolves everything by `orderId`.

use std::time::Duration;

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::actions::{dispatch, ActionError, DispatchHandle};
use crate::generated::data_layer::{ActionKey, ResolverKey};
use crate::graphql::{execute_resolver, Transport};

/// How many `paymentStatus.byOrder` reads [`PlacedOrder::await_payment_intent`] makes before giving
/// up, and their spacing. Same shape (and rationale) as the dispatcher's poll bounds: intent
/// creation is one saga leg + one Stripe call — seconds, not minutes; an intent that never appears
/// within the bound means the saga rejected/failed, and the operationStatus read carries why.
pub const INTENT_POLL_MAX_ATTEMPTS: u32 = 30;
pub const INTENT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// What checkout collects from the customer (the `contact_form` + delivery inputs of the screen
/// spec). Plain data — the Leptos form binds onto it; tests build it literally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutForm {
    pub full_name: String,
    /// The VERIFIED phone (the form field is `disabled: true` — identity comes from the auth
    /// layer's late identification at the cart→checkout boundary, never free text).
    pub phone: String,
    pub email: Option<String>,
    /// `DELIVERY` or `COLLECTION` (`scalars.yaml#/ServiceType`).
    pub service_type: String,
    /// Required when `service_type == DELIVERY` (`entities.yaml#/Address` shape).
    pub delivery_address: Option<Value>,
    /// Delivery instructions (`OrderNote`).
    pub note: Option<String>,
}

/// The checkout context the screen already holds when the form submits (from `cart.current` +
/// `restaurant.bySlug` + the auth session).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutContext {
    pub restaurant_id: String,
    pub cart_id: String,
    /// Bound when the customer is authenticated; `PlaceOrder.customerId` is nullable by spec.
    pub customer_id: Option<String>,
    /// The total the UI displayed — sent as `expectedTotal` so the server can reject a
    /// `PriceMismatch` instead of ever charging an amount the customer did not see.
    pub expected_total: Option<Value>,
}

/// Everything checkout can fail with beyond the dispatcher's own errors.
#[derive(Debug, thiserror::Error)]
pub enum CheckoutError {
    #[error(transparent)]
    Action(#[from] ActionError),
    #[error(transparent)]
    Resolver(#[from] crate::graphql::ResolverError),
    /// The intent read never produced a clientSecret within the bound — resolve the command's own
    /// verdict via `operationStatus` (the saga has rejected or failed; that read says why).
    #[error("no payment intent for order {order_id} after {attempts} reads — check operationStatus for the verdict")]
    IntentUnavailable { order_id: Uuid, attempts: u32 },
}

/// A Stripe PaymentIntent as `paymentStatus.byOrder` exposes it (`{ paymentIntentId clientSecret
/// status }`). `client_secret` is the ONLY payment credential the client ever sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentIntent {
    pub payment_intent_id: Option<String>,
    pub client_secret: Option<String>,
    pub status: Option<String>,
}

impl PaymentIntent {
    fn parse(v: &Value) -> Self {
        let field = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        Self {
            payment_intent_id: field("paymentIntentId"),
            client_secret: field("clientSecret"),
            status: field("status"),
        }
    }
}

/// The accepted checkout: the minted ids + the acceptance handle. Deliberately plain data, like
/// [`DispatchHandle`] — persisting `order_id` (the route) and `handle.message_id` is what makes the
/// flow reload-proof.
#[derive(Debug, Clone)]
pub struct PlacedOrder {
    pub order_id: Uuid,
    pub handle: DispatchHandle,
}

impl PlacedOrder {
    /// The confirmation route (`screens/order_tracking.route`) — known client-side at mint time,
    /// which is exactly why the spec makes `orderId` client-generated.
    pub fn confirmation_route(&self) -> String {
        format!("/orders/{}/confirmation", self.order_id)
    }

    /// Await the Stripe intent for this order with the production bounds.
    pub async fn await_payment_intent(
        &self,
        transport: &dyn Transport,
    ) -> Result<PaymentIntent, CheckoutError> {
        self.await_payment_intent_with(transport, INTENT_POLL_MAX_ATTEMPTS, INTENT_POLL_INTERVAL)
            .await
    }

    /// Await with explicit bounds (tests pass `Duration::ZERO`). A `null` intent or one without a
    /// `clientSecret` yet means "saga still working" — keep reading until the bound; the
    /// `paymentStatusChanged` subscription is the push path for the same row and needs no bound.
    pub async fn await_payment_intent_with(
        &self,
        transport: &dyn Transport,
        max_attempts: u32,
        interval: Duration,
    ) -> Result<PaymentIntent, CheckoutError> {
        let mut vars = Map::new();
        vars.insert("orderId".into(), json!(self.order_id));
        for attempt in 1..=max_attempts {
            if attempt > 1 && !interval.is_zero() {
                crate::actions::sleep(interval).await;
            }
            let intent =
                execute_resolver(transport, ResolverKey::PaymentStatusByOrder, vars.clone()).await?;
            if intent.is_null() {
                continue;
            }
            let parsed = PaymentIntent::parse(&intent);
            if parsed.client_secret.is_some() {
                return Ok(parsed);
            }
        }
        Err(CheckoutError::IntentUnavailable { order_id: self.order_id, attempts: max_attempts })
    }
}

/// Submit the checkout: mint the `orderId`, assemble the `PlaceOrder` input per the command spec,
/// and dispatch it acceptance-first. Returns as soon as the command is journaled — the caller
/// navigates to [`PlacedOrder::confirmation_route`] and resolves payment/outcome from there.
///
/// `payment_method_id` comes from the Stripe element (`stripe.rs`) — the wallet behind it is a
/// Stripe concern, the domain only carries the reference.
pub async fn submit(
    transport: &dyn Transport,
    ctx: &CheckoutContext,
    form: &CheckoutForm,
    payment_method_id: &str,
) -> Result<PlacedOrder, CheckoutError> {
    let order_id = Uuid::now_v7();
    let input = place_order_input(ctx, form, payment_method_id, order_id);
    let handle = dispatch(transport, ActionKey::PlaceOrder, input).await?;
    Ok(PlacedOrder { order_id, handle })
}

/// The `PlaceOrder` input per the command spec — shared by [`submit`] and [`submit_persisted`].
fn place_order_input(
    ctx: &CheckoutContext,
    form: &CheckoutForm,
    payment_method_id: &str,
    order_id: Uuid,
) -> Map<String, Value> {
    let mut contact = Map::new();
    contact.insert("displayName".into(), json!(form.full_name));
    contact.insert("phone".into(), json!(form.phone));
    if let Some(email) = &form.email {
        contact.insert("email".into(), json!(email));
    }

    let mut input = Map::new();
    input.insert("orderId".into(), json!(order_id));
    input.insert("restaurantId".into(), json!(ctx.restaurant_id));
    input.insert("cartId".into(), json!(ctx.cart_id));
    if let Some(customer_id) = &ctx.customer_id {
        input.insert("customerId".into(), json!(customer_id));
    }
    input.insert("customerContact".into(), Value::Object(contact));
    input.insert("serviceType".into(), json!(form.service_type));
    if let Some(address) = &form.delivery_address {
        input.insert("deliveryAddress".into(), address.clone());
    }
    if let Some(note) = &form.note {
        input.insert("note".into(), json!(note));
    }
    input.insert("paymentMethodId".into(), json!(payment_method_id));
    if let Some(total) = &ctx.expected_total {
        input.insert("expectedTotal".into(), total.clone());
    }
    input
}

/// [`submit`] with the intent PERSISTED first (#17, `pending.rs`): the recorded input carries the
/// client-minted `orderId`, so after a mid-checkout reload the resume path recovers BOTH the
/// idempotency id (safe retry, no double order) AND the confirmation route — the full #12
/// continuity story for the money mutation. Clearing happens via `pending::settle` (or the boot
/// resume), never implicitly.
pub async fn submit_persisted(
    transport: &dyn Transport,
    store: &dyn crate::pending::PendingStore,
    ctx: &CheckoutContext,
    form: &CheckoutForm,
    payment_method_id: &str,
) -> Result<PlacedOrder, CheckoutError> {
    let order_id = Uuid::now_v7();
    let input = place_order_input(ctx, form, payment_method_id, order_id);
    let handle =
        crate::pending::dispatch_persisted(transport, store, ActionKey::PlaceOrder, input).await?;
    Ok(PlacedOrder { order_id, handle })
}

// ─── The checkout screen (Leptos, SSR + hydrate from the same tree) ────────────────────────────

use leptos::prelude::*;

/// What the checkout page renders from (resolved by the SSR/hydrate shells via `cart.current` +
/// `me.profile` before mounting). Amounts arrive pre-formatted — money formatting stays one layer
/// up with the resolver data, not in the view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutViewState {
    pub restaurant_name: String,
    pub cart_line_count: usize,
    pub formatted_total: String,
    pub is_delivery: bool,
}

/// The checkout screen — the spec's component tree (`back_button_header`, the `checkout_section`s,
/// the Stripe mount, `sticky_bottom_bar`) rendered with the same `data-c` tagging discipline as the
/// SDUI renderer, so the DOM stays auditable against the spec even off-registry. Interactive form
/// state arrives with the router/mount plumbing (split 4); this tree is the SSR shape both builds
/// share.
#[component]
pub fn CheckoutScreen(state: CheckoutViewState) -> impl IntoView {
    let summary = format!("{} items - {}", state.cart_line_count, state.formatted_total);
    view! {
        <main id="app" data-hydrate="checkout">
            <header data-c="back_button_header"><h1>"Checkout"</h1></header>
            {state.is_delivery.then(|| view! {
                <section data-c="checkout_section" data-s="delivery_details">
                    <div data-c="address_selector" id="delivery_address"></div>
                </section>
            })}
            <section data-c="checkout_section" data-s="contact">
                <form data-c="form" id="contact_form">
                    <input data-c="text_input" id="full_name" required/>
                    <input data-c="phone_input" id="phone" disabled/>
                    <input data-c="email_input" id="email"/>
                </form>
            </section>
            <section data-c="checkout_section" data-s="order_summary">
                <div data-c="cart_summary_mini">{summary}" from "{state.restaurant_name}</div>
            </section>
            <section data-c="checkout_section" data-s="payment">
                // The Stripe mount point: stripe.rs attaches the element here on hydrate.
                <div data-c="stripe_express_checkout_element" id=crate::stripe::MOUNT_ID></div>
            </section>
            <footer data-c="sticky_bottom_bar">
                <button data-c="button" id="place_order_btn" data-variant="primary">
                    "Place order - "{state.formatted_total}
                </button>
            </footer>
        </main>
    }
}

/// Server-side render the checkout page to a full document (the `ssr` build).
#[cfg(feature = "ssr")]
pub fn render_checkout_html(state: CheckoutViewState) -> String {
    let body = CheckoutScreen(CheckoutScreenProps { state }).to_html();
    crate::renderer::page_html("Checkout - Captain.Food", &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::ActionOutcome;
    use crate::graphql::test_support::FakeTransport;

    fn ctx() -> CheckoutContext {
        CheckoutContext {
            restaurant_id: "rest-1".into(),
            cart_id: "cart-1".into(),
            customer_id: Some("cust-1".into()),
            expected_total: Some(json!({ "amountCents": 2350, "currency": "EUR" })),
        }
    }

    fn form() -> CheckoutForm {
        CheckoutForm {
            full_name: "Jo Dupont".into(),
            phone: "+33600000000".into(),
            email: None,
            service_type: "DELIVERY".into(),
            delivery_address: Some(json!({
                "line1": "1 rue Nationale", "postalCode": "37000", "city": "Tours", "country": "FR",
            })),
            note: Some("Ring twice".into()),
        }
    }

    fn acceptance(status: &str) -> Value {
        json!({ "placeOrder": {
            "messageId": "00000000-0000-7000-8000-000000000000",
            "correlationId": "00000000-0000-7000-8000-000000000000",
            "causeId": null, "sessionId": null, "traceId": null,
            "operationStatus": status, "duplicate": false,
        }})
    }

    #[tokio::test]
    async fn submit_mints_the_order_id_and_builds_the_place_order_input() {
        let fake = FakeTransport::scripted(vec![Ok(acceptance("PENDING"))]);
        let placed = submit(&fake, &ctx(), &form(), "pm_123").await.unwrap();

        // The route is known from the client-minted id — before any server verdict.
        assert_eq!(placed.order_id.get_version_num(), 7);
        assert_eq!(placed.confirmation_route(), format!("/orders/{}/confirmation", placed.order_id));

        let (document, variables) = fake.call(0);
        assert!(document.contains("placeOrder(input: $input, metadata: $metadata)"), "{document}");
        let input = &variables["input"];
        assert_eq!(input["orderId"], json!(placed.order_id));
        assert_eq!(input["restaurantId"], "rest-1");
        assert_eq!(input["cartId"], "cart-1");
        assert_eq!(input["customerId"], "cust-1");
        assert_eq!(input["customerContact"]["displayName"], "Jo Dupont");
        assert_eq!(input["customerContact"]["phone"], "+33600000000");
        assert!(input["customerContact"].get("email").is_none(), "absent email stays absent");
        assert_eq!(input["serviceType"], "DELIVERY");
        assert_eq!(input["deliveryAddress"]["city"], "Tours");
        assert_eq!(input["note"], "Ring twice");
        assert_eq!(input["paymentMethodId"], "pm_123");
        // The displayed total travels — the server's PriceMismatch guard depends on it.
        assert_eq!(input["expectedTotal"]["amountCents"], 2350);
    }

    #[tokio::test]
    async fn await_payment_intent_polls_until_the_client_secret_exists() {
        let fake = FakeTransport::scripted(vec![
            Ok(acceptance("PENDING")),
            // Saga not there yet: null row, then a row without a secret, then the real intent.
            Ok(json!({ "paymentStatus": null })),
            Ok(json!({ "paymentStatus": { "paymentIntentId": null, "clientSecret": null, "status": null } })),
            Ok(json!({ "paymentStatus": { "paymentIntentId": "pi_1", "clientSecret": "pi_1_secret_x", "status": "REQUIRES_CONFIRMATION" } })),
        ]);
        let placed = submit(&fake, &ctx(), &form(), "pm_123").await.unwrap();
        let intent =
            placed.await_payment_intent_with(&fake, 5, Duration::ZERO).await.unwrap();
        assert_eq!(intent.client_secret.as_deref(), Some("pi_1_secret_x"));
        // Every poll queried OUR order.
        assert_eq!(fake.call(1).1["input"]["orderId"], json!(placed.order_id));
    }

    #[tokio::test]
    async fn intent_polling_gives_up_at_the_bound() {
        let fake = FakeTransport::scripted(vec![
            Ok(acceptance("PENDING")),
            Ok(json!({ "paymentStatus": null })),
            Ok(json!({ "paymentStatus": null })),
        ]);
        let placed = submit(&fake, &ctx(), &form(), "pm_123").await.unwrap();
        let err = placed.await_payment_intent_with(&fake, 2, Duration::ZERO).await.unwrap_err();
        assert!(matches!(err, CheckoutError::IntentUnavailable { attempts: 2, .. }));
    }

    #[tokio::test]
    async fn a_rejected_checkout_resolves_as_a_business_rejection_not_an_error() {
        // The two-step contract on the money path: PriceMismatch arrives as REJECTED with its
        // errors.yaml code — normal UX flow (re-quote the cart), never an exception.
        let fake = FakeTransport::scripted(vec![
            Ok(acceptance("PENDING")),
            Ok(json!({ "operationStatus": {
                "messageId": "00000000-0000-7000-8000-000000000000",
                "correlationId": "00000000-0000-7000-8000-000000000000",
                "status": "REJECTED", "errorCode": "PriceMismatch",
                "message": "The displayed total is stale", "occurredAt": "2026-07-23T12:00:00Z",
            }})),
        ]);
        let placed = submit(&fake, &ctx(), &form(), "pm_123").await.unwrap();
        match placed.handle.resolve_with(&fake, 5, Duration::ZERO).await.unwrap() {
            ActionOutcome::Rejected { error_code, .. } => assert_eq!(error_code, "PriceMismatch"),
            other => panic!("expected the anticipated rejection, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn persisted_submit_records_the_order_id_so_a_reload_recovers_the_route() {
        use crate::graphql::TransportError;
        use crate::pending::{MemoryPendingStore, PendingStore};

        let store = MemoryPendingStore::default();
        // The worst moment: the radio dies ON the placeOrder send.
        let fake = FakeTransport::scripted(vec![Err(TransportError::Network("tab killed".into()))]);
        let err = submit_persisted(&fake, &store, &ctx(), &form(), "pm_123").await.unwrap_err();
        assert!(matches!(err, CheckoutError::Action(crate::actions::ActionError::Transport(_))));

        // The record alone recovers BOTH halves of #12's continuity story:
        let write = store.load().remove(0);
        let order_id = write.input["orderId"].as_str().unwrap().to_string();
        assert_eq!(format!("/orders/{order_id}/confirmation"), {
            let id = uuid::Uuid::parse_str(&order_id).unwrap();
            PlacedOrder {
                order_id: id,
                handle: crate::actions::DispatchHandle {
                    message_id: write.message_id,
                    duplicate: true,
                    status_at_acceptance: crate::actions::OperationStatus::Pending,
                },
            }
            .confirmation_route()
        });
        // ...and the retry re-sends the SAME messageId with the SAME orderId — no double order.
        let fake = FakeTransport::scripted(vec![Ok(acceptance("PENDING"))]);
        crate::pending::retry(&fake, &write).await.unwrap();
        let sent = fake.call(0).1;
        assert_eq!(sent["metadata"]["messageId"], serde_json::json!(write.message_id.to_string()));
        assert_eq!(sent["input"]["orderId"], serde_json::json!(order_id));
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn checkout_renders_the_spec_component_tree() {
        let html = render_checkout_html(CheckoutViewState {
            restaurant_name: "Chez Test".into(),
            cart_line_count: 2,
            formatted_total: "23,50 EUR".into(),
            is_delivery: true,
        });
        for tag in [
            "back_button_header",
            "checkout_section",
            "cart_summary_mini",
            "stripe_express_checkout_element",
            "sticky_bottom_bar",
        ] {
            assert!(html.contains(&format!("data-c=\"{tag}\"")), "missing {tag}: {html}");
        }
        assert!(html.contains("data-hydrate=\"checkout\""));
        assert!(html.contains(crate::stripe::MOUNT_ID), "the Stripe mount point must exist");
        // COLLECTION hides the delivery sections.
        let collection = render_checkout_html(CheckoutViewState {
            restaurant_name: "Chez Test".into(),
            cart_line_count: 1,
            formatted_total: "9,80 EUR".into(),
            is_delivery: false,
        });
        assert!(!collection.contains("data-s=\"delivery_details\""));
    }
}
