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
    /// The total the UI displayed — DEPRECATED (`rules.yaml#/ServerPriceAuthority`,
    /// PROP-20260831-134539 slice 3b): `errors.yaml#/PriceMismatch` (the error this field's
    /// equality check used to trigger) is RETIRED, and the server now ignores this field
    /// entirely, on either door state. Superseded by the signed `quote` (D-A/D-J, checkpoint 2
    /// item (1)); still sent for observability/legacy-fixture reasons only.
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

    /// The terminal failure the customer must be TOLD about: the saga's FAILED leg (PaymentFailed
    /// recorded inbound from Stripe → the run row goes FAILED, nothing is placed and the cart stays
    /// OPEN). This is what drives the checkout screen's `payment_failed_state`
    /// (specs/screens/restaurant_frontoffice.yaml, #424 quick win 2).
    pub fn is_failed(&self) -> bool {
        self.status.as_deref() == Some(FAILED_PAYMENT_STATUS)
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
    ///
    /// A **FAILED** status short-circuits the poll and returns `Ok` (#424 quick win 2): the payment
    /// is terminally refused, so no `clientSecret` is ever coming and polling out the whole bound
    /// would leave the customer on a spinner at the peak of the anxiety curve — the exact failure
    /// this quick win exists to remove. The caller renders `payment_failed_state` from
    /// [`PaymentIntent::is_failed`]; the cart is untouched, so retry is safe.
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
            if parsed.client_secret.is_some() || parsed.is_failed() {
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
    /// `paymentStatus.byOrder.status == FAILED` ([`PaymentIntent::is_failed`]) — renders the
    /// screen's `payment_failed_state` (#424 quick win 2). False on the data-less SSR shell: a
    /// customer who has not paid yet must never be shown a failure.
    ///
    /// LAYER NOTE (evans, #730 review): "failed" appears twice on this struct, one line apart,
    /// in TWO different layers — this field is the DOMAIN outcome (the read ANSWERED, and what
    /// it said is that the payment terminally failed); `payment_status_failed` below is the READ
    /// mark (the transport/contract failed, nothing answered at all). Conflating them is the
    /// "Commande introuvable over a transient failure" defect class.
    pub payment_failed: bool,
    /// The locale the failure copy resolves in. The rest of this shell is still hardcoded English
    /// (a pre-existing gap); the failure copy is translated because it is the one place on this
    /// page where the wrong language costs a conversion at the worst possible moment.
    pub locale: String,
    /// The mountable Stripe publishable key (#440), or `None` = the presence-gated degraded state:
    /// the payment section renders `payment_unavailable_state` INSTEAD of the element (a dead
    /// payment control is worse than no control), and `place_order_btn` is disabled. Set from the
    /// render context by [`with_publishable_key`](Self::with_publishable_key) — it is a deployment
    /// fact, never resolver data.
    pub publishable_key: Option<crate::stripe::PublishableKey>,
    /// `cart.current` FAILED (#730 — transport/contract failure, never an answered-empty cart):
    /// the order summary renders the error affordance instead of blank money, and
    /// `place_order_btn` disables (its `{{ cart.id }}` variable binds the failed read — an order
    /// priced on data the customer never saw).
    pub cart_failed: bool,
    /// `restaurant.bySlug` FAILED: `place_order_btn` disables (`{{ restaurant.id }}`); nothing
    /// else on this page displays that read, so no error section renders for it.
    pub restaurant_failed: bool,
    /// `me.profile` FAILED: the contact fields stay EMPTY-EDITABLE (an error state blocking
    /// manual entry would hide the working path) with a small section notice.
    pub profile_failed: bool,
    /// `paymentStatus.byOrder` FAILED. Since #745 the PAINT loops never set this: the read is a
    /// declared structurally-unfulfillable skip (`skipped_reads` on the checkout screen — its
    /// required `orderId` is client-minted at dispatch), skipped before any network, so the
    /// #730 "failed on every anonymous first paint" era is over. Kept, still deliberately NOT a
    /// degrade gate: gating `payment_unavailable_state` on a payment-status read failure would
    /// close checkout over a cosmetic read — the exact failure the business rule forbids — and
    /// the flag becomes reachable again the day the read carries its arg at paint time.
    pub payment_status_failed: bool,
}

impl CheckoutViewState {
    /// Build from an already-RESOLVED render context — the screen's OWN declared resolvers
    /// (`cart.current`, `me.profile`, `paymentStatus.byOrder`), resolved by whichever entry is
    /// rendering. Before #420 the only production call site hardcoded `restaurant_name: ""`,
    /// `cart_line_count: 0`, `formatted_total: ""` and `payment_failed: false`, so a customer at
    /// the moment of paying saw an empty summary and the failure state was unreachable outside a
    /// unit test (PROP-20260809-021351 §2, G5).
    ///
    /// `restaurant_fallback` is the storefront's tenant label from the HOST — used only because the
    /// `cart.current` selection carries `restaurantId` and no NAME. It is true (the customer is on
    /// that restaurant's storefront) but it is a slug, not a display name; getting the real one
    /// needs the checkout screen's `resolvers` (or the Cart type) to carry it, which is a DSL change
    /// and is reported on #420 rather than smuggled in here.
    pub fn from_context(
        ctx: &crate::renderer::RenderContext,
        restaurant_fallback: Option<&str>,
        locale: &str,
    ) -> Self {
        // Through the context's own binding resolution (#729: data is keyed by full resolver
        // key; `cart` / `paymentStatus` are the template aliases of `cart.current` /
        // `paymentStatus.byOrder`), so this shell and the SDUI renderer can never disagree on
        // which resolver feeds a name.
        let cart_value = ctx.binding_json("cart").filter(|v| !v.is_null());
        let cart = cart_value.as_ref();
        let str_at = |v: Option<&Value>, path: &[&str]| -> Option<String> {
            let mut cur = v?;
            for seg in path {
                cur = cur.get(seg)?;
            }
            cur.as_str().map(str::to_string)
        };
        let restaurant_name = str_at(cart, &["restaurantName"])
            .or_else(|| str_at(cart, &["restaurant", "displayName"]))
            .or_else(|| restaurant_fallback.map(str::to_string))
            .unwrap_or_default();
        let cart_line_count = cart
            .and_then(|c| c.get("lines"))
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        let formatted_total = cart
            .and_then(|c| c.get("totalAmount"))
            .map(crate::renderer::format_currency)
            .unwrap_or_default();
        // Absent `serviceType` means DELIVERY: the delivery sections are the fuller form, and
        // hiding an address field a customer needs is the costlier of the two mistakes.
        let is_delivery = str_at(cart, &["serviceType"]).as_deref() != Some("COLLECTION");
        // The terminal refusal, from the screen's own `paymentStatus.byOrder` requirement — the
        // ONE thing on this page a customer must never be left guessing about.
        let payment_status = ctx.binding_json("paymentStatus");
        let payment_failed = str_at(payment_status.as_ref(), &["status"]).as_deref()
            == Some(FAILED_PAYMENT_STATUS);
        Self {
            restaurant_name,
            cart_line_count,
            formatted_total,
            is_delivery,
            payment_failed,
            locale: locale.to_string(),
            publishable_key: None,
            // #730: failed ≠ unresolved — only a read `classify_resolve` marked FAILED degrades
            // a section; the anonymous-SSR skip keeps today's empty shell.
            cart_failed: ctx.binding_failed("cart"),
            restaurant_failed: ctx.binding_failed("restaurant"),
            profile_failed: ctx.binding_failed("me"),
            payment_status_failed: ctx.binding_failed("paymentStatus"),
        }
    }

    /// Attach the delivery-seam fact (#440): the parsed publishable key from the render context.
    /// Separate from [`from_resolved`](Self::from_resolved) because the key is server
    /// configuration riding the RenderContext, not data any resolver returns.
    pub fn with_publishable_key(
        mut self,
        key: Option<crate::stripe::PublishableKey>,
    ) -> Self {
        self.publishable_key = key;
        self
    }
}

/// The `paymentStatus.byOrder.status` token that means "terminally refused" — one spelling, shared
/// by [`PaymentIntent::is_failed`] and [`CheckoutViewState::from_resolved`].
const FAILED_PAYMENT_STATUS: &str = "FAILED";

/// The checkout screen — the spec's component tree (`back_button_header`, the `checkout_section`s,
/// the Stripe mount, `sticky_bottom_bar`) rendered with the same `data-c` tagging discipline as the
/// SDUI renderer, so the DOM stays auditable against the spec even off-registry. Interactive form
/// state arrives with the router/mount plumbing (split 4); this tree is the SSR shape both builds
/// share.
#[component]
pub fn CheckoutScreen(state: CheckoutViewState) -> impl IntoView {
    let summary = format!("{} items - {}", state.cart_line_count, state.formatted_total);
    // #817 (C. conso. L221-14 / CRD 2011/83 Art. 8(2)): the pay button must carry an unambiguous
    // mention of the OBLIGATION TO PAY, not merely the amount. The copy is deliberately NOT written
    // here -- it resolves from the generated catalog, so there is ONE source for this label and
    // `specs/screens/restaurant_frontoffice.translations.yaml` is it. The literal this replaces,
    // `"Place order - "{total}`, was doubly wrong: it stated an amount rather than an obligation,
    // and it rendered ENGLISH to every customer including the French ones the article reaches --
    // and this screen is `sdui: false`, so nothing else held the declaration and the runtime
    // together.
    //
    // Be accurate about the level (PROP-20260802-130500 §1, ADR-20260803-234035): this is **level
    // 3**, not the compiler-first level 4. The key below is a hand-authored `&str` -- `"checkout.
    // place_ordr"` compiles fine and renders i18n's fail-visible `[key]` marker -- and nothing
    // proves code-to-catalog at build time. What it buys is single source + a fail-visible fallback
    // + the two mutation-checked tests below, which is genuinely stronger than a parity assertion
    // between two hand-written strings, and is worth claiming at its real height and no higher.
    // Reaching level 4 would need generated key constants; that is not this change.
    let place_order_label = crate::i18n::format_message(
        "checkout.place_order",
        &state.locale,
        &[("total", &state.formatted_total)],
    );
    let locale = state.locale.clone();
    let t = move |key: &str| crate::i18n::resolve(key, &locale);
    // The delivery seam's last hop (#440): the key rides a DATA ATTRIBUTE on the mount div (the
    // hydrate path reads it back via stripe::MOUNT_KEY_ATTR — pinned to the literal below by
    // `stripe::tests::the_mount_id_is_a_stable_dom_contract`). No key ⇒ no element, no dead
    // control: the payment section renders `payment_unavailable_state` and the pay button is
    // disabled. `payment_unavailable` also covers a browser-side degrade (stripe.js failed) —
    // the hydrate mount flips the same state rather than inventing a second one.
    // NOTE (#730): `payment_status_failed` is deliberately NOT part of this gate — see the field
    // docs: the read fails structurally on every anonymous paint (arg-less by route), so keying
    // the degrade on it would close checkout permanently.
    let payment_unavailable = state.publishable_key.is_none();
    let mount_key = state.publishable_key.as_ref().map(|k| k.as_str().to_string());
    let test_mode = state.publishable_key.as_ref().is_some_and(|k| k.is_test_mode());
    // #730 (ux + legal): `place_order`'s variables bind `{{ cart.id }}` and `{{ restaurant.id }}`
    // — a failed read of either disables the pay button (an order priced on data the customer
    // never saw), with the translated reason as its tooltip. Merely-unresolved data keeps
    // today's behaviour.
    let data_failed = state.cart_failed || state.restaurant_failed;
    let place_order_disabled = payment_unavailable || data_failed;
    let disabled_reason =
        data_failed.then(|| crate::i18n::resolve("common.error.data_unavailable", &state.locale));
    let summary_failed = state.cart_failed;
    let profile_failed = state.profile_failed;
    let summary_error_copy = crate::i18n::resolve("common.error.data_unavailable", &state.locale);
    let retry_copy = crate::i18n::resolve("common.error.retry", &state.locale);
    let notice_copy = summary_error_copy.clone();
    view! {
        <main id="app" data-hydrate="checkout">
            <header data-c="back_button_header"><h1>"Checkout"</h1></header>
            {state.is_delivery.then(|| view! {
                <section data-c="checkout_section" data-s="delivery_details">
                    <div data-c="address_selector" id="delivery_address"></div>
                </section>
            })}
            <section data-c="checkout_section" data-s="contact">
                // #730: a failed `me.profile` read leaves the fields EMPTY-EDITABLE — the manual
                // path keeps working — with a small notice, never a section-replacing error.
                {profile_failed.then(|| view! {
                    <p data-c="text" id="contact_prefill_notice" data-error="true" data-size="sm">
                        {notice_copy.clone()}
                    </p>
                })}
                <form data-c="form" id="contact_form">
                    <input data-c="text_input" id="full_name" required/>
                    <input data-c="phone_input" id="phone" disabled/>
                    <input data-c="email_input" id="email"/>
                </form>
            </section>
            // #817 (C. conso. L221-14 / CRD 2011/83 Art. 8(2)): the recap of the total shown
            // immediately before ordering must be CLEAR AND LEGIBLE, so this section renders
            // EXPANDED -- no collapse affordance, the total is in the first paint. The screen
            // declared `collapsible: true` until #817 and no renderer ever read the flag, which is
            // why the guarantee lives in `the_total_recap_is_visible_before_ordering_...` below.
            <section data-c="checkout_section" data-s="order_summary">
                // #730: a FAILED cart read renders the #472 error affordance — never blank money
                // in the summary a customer is about to pay against.
                {if summary_failed { view! {
                    <div data-c="cart_summary_mini" data-error="true">
                        <p>{summary_error_copy.clone()}</p>
                        <a href="" data-c="retry_button" role="button">{retry_copy.clone()}</a>
                    </div>
                }.into_any() } else { view! {
                    <div data-c="cart_summary_mini">{summary}" from "{state.restaurant_name}</div>
                }.into_any() }}
            </section>
            <section data-c="checkout_section" data-s="payment">
                // The Stripe mount point: stripe.rs attaches the element here on hydrate, reading
                // the publishable key off the `data-pk` attribute. Rendered ONLY when a mountable
                // key exists — an element div that nothing can ever mount is a dead control.
                {mount_key.map(|pk| view! {
                    <div data-c="stripe_express_checkout_element" id=crate::stripe::MOUNT_ID data-pk=pk></div>
                })}
                // Authorize-then-capture disclosure (#544): the card is HELD now and DEBITED on
                // delivery. Persistent on the pay step — framing the hold as a charge is wrong.
                <p data-c="text" id="payment_hold_notice" data-size="sm">{t("checkout.payment.hold_notice")}</p>
                // Test-mode banner (#440): persistent on the pay step while the key is pk_test_.
                {test_mode.then(|| view! {
                    <p data-c="text" id="test_mode_banner" data-size="sm">{t("checkout.test_mode.banner")}</p>
                })}
                // `payment_unavailable_state` (spec: screens/restaurant_frontoffice.yaml#checkout,
                // #440): the presence-gated degrade — no usable key reached the shell (or stripe.js
                // failed browser-side). "Your cart is saved" is a promise the system keeps: nothing
                // was placed, the cart stays OPEN. Counted server-side as
                // checkout_degraded_render_total{reason=stripe_key_absent}; the pay button below is
                // disabled in the same state.
                {payment_unavailable.then(|| view! {
                    <section data-c="conditional_section" id="payment_unavailable_state">
                        <p data-c="text" data-weight="bold">{t("checkout.payment_unavailable.title")}</p>
                        <p data-c="text">{t("checkout.payment_unavailable.body")}</p>
                        <button
                            data-c="button"
                            id="payment_unavailable_back_btn"
                            data-variant="outline"
                            data-action="navigate"
                            data-route="/cart"
                        >
                            {t("checkout.payment_failed.back_to_cart")}
                        </button>
                    </section>
                })}
            </section>
            // `payment_failed_state` (spec: screens/restaurant_frontoffice.yaml#checkout). The saga
            // keeps the cart OPEN and places nothing on PaymentFailed, so "your cart is intact" is
            // a promise the system actually keeps. Both actions are client-kind `navigate`, carried
            // by the same `data-action`/`data-route` DOM contract the SDUI renderer emits.
            // WIRED since #420, NOT YET REACHABLE — the distinction matters, and an earlier draft
            // of this comment got it wrong (caught by the #427 mob review). Production now derives
            // `payment_failed` from the screen's own `paymentStatus.byOrder` requirement
            // (`CheckoutViewState::from_resolved`) instead of hardcoding `false`, and `hydrate()`
            // mounts this screen and installs the delegated listener. But `paymentStatus` takes a
            // REQUIRED `orderId` and `/checkout` carries no route params, so the read is dropped
            // before it is sent and `payment_failed` cannot become true. Reaching this state needs
            // two DSL changes reported on #420: a way for the route to supply the order id, and a
            // browser-side Stripe publishable key (no such configuration key exists anywhere).
            // Neither is worked around here.
            {state.payment_failed.then(|| view! {
                <section data-c="conditional_section" id="payment_failed_state">
                    <p data-c="text" data-size="xl" data-weight="bold" data-color="error">
                        {t("checkout.payment_failed.title")}
                    </p>
                    <p data-c="text">{t("checkout.payment_failed.body")}</p>
                    <button
                        data-c="button"
                        id="retry_payment_btn"
                        data-variant="primary"
                        data-action="navigate"
                        data-route="/checkout"
                    >
                        {t("checkout.payment_failed.retry")}
                    </button>
                    <button
                        data-c="button"
                        id="back_to_cart_btn"
                        data-variant="outline"
                        data-action="navigate"
                        data-route="/cart"
                    >
                        {t("checkout.payment_failed.back_to_cart")}
                    </button>
                </section>
            })}
            <footer data-c="sticky_bottom_bar">
                // #730 (business lens, checkpoint): the disable reason must be VISIBLE — a
                // `title` tooltip is invisible on mobile touch, our primary surface. Same shape
                // as the contact_prefill_notice above; covers the restaurant-only failure, where
                // no summary error renders (that read has no display binding on this page).
                {data_failed.then(|| view! {
                    <p data-c="text" id="place_order_notice" data-error="true" data-size="sm">
                        {notice_copy.clone()}
                    </p>
                })}
                // Disabled while payment is unavailable (#440: a pay button over an unmountable
                // element would place an order nothing can pay for) or while the cart/restaurant
                // read FAILED (#730: an order priced on data the customer never saw).
                <button
                    data-c="button"
                    id="place_order_btn"
                    data-variant="primary"
                    disabled=place_order_disabled
                    title=disabled_reason
                >
                    {place_order_label}
                </button>
            </footer>
        </main>
    }
}

/// Server-side render the checkout page to a full document (the `ssr` build).
///
/// The stripe.js `<script>` tag is injected into THIS shell only, and only when a mountable key
/// exists (#440 — checkout-only over every-page: privacy over Stripe's fraud-signal preference;
/// no key, no Stripe request leaves the customer's browser at all).
#[cfg(feature = "ssr")]
pub fn render_checkout_html(mut state: CheckoutViewState, lang: &str) -> String {
    let lang = crate::i18n::normalize_locale(lang).unwrap_or(crate::i18n::DEFAULT_LOCALE);
    // ONE source of truth for the language: the document's `lang` is also what the copy resolves in.
    state.locale = lang.to_string();
    let with_stripe_js = state.publishable_key.is_some();
    let body = CheckoutScreen(CheckoutScreenProps { state }).to_html();
    let doc = crate::renderer::page_html("Checkout - Captain.Food", lang, &body);
    if with_stripe_js {
        doc.replace("</head>", &format!("{}</head>", crate::stripe::STRIPE_JS_TAG))
    } else {
        doc
    }
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
        // The displayed total still travels on the wire (retargeted, checkpoint 2 item I): the
        // server IGNORES this field entirely now (errors.yaml#/PriceMismatch retired) -- dropping
        // it from the wire is checkpoint 2's separate item (1) (D-J client minimum), not this fix.
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

    /// #424 quick win 2: a refused payment must not be answered with a spinner. The FAILED status
    /// ENDS the poll on the read that reports it (no clientSecret is ever coming), so the page can
    /// render `payment_failed_state` immediately instead of waiting out the whole bound and then
    /// showing a technical `IntentUnavailable`.
    #[tokio::test]
    async fn a_failed_payment_ends_the_poll_immediately_instead_of_spinning() {
        let fake = FakeTransport::scripted(vec![
            Ok(acceptance("PENDING")),
            Ok(json!({ "paymentStatus": { "paymentIntentId": "pi_1", "clientSecret": null, "status": null } })),
            Ok(json!({ "paymentStatus": { "paymentIntentId": "pi_1", "clientSecret": null, "status": "FAILED" } })),
            // Never reached: the poll stops on FAILED rather than running to the bound.
            Ok(json!({ "paymentStatus": { "paymentIntentId": "pi_1", "clientSecret": "late_secret", "status": "FAILED" } })),
        ]);
        let placed = submit(&fake, &ctx(), &form(), "pm_123").await.unwrap();
        let intent = placed.await_payment_intent_with(&fake, 5, Duration::ZERO).await.unwrap();
        assert!(intent.is_failed(), "the page learns the payment failed");
        assert_eq!(intent.client_secret, None);
        assert_eq!(fake.call_count(), 3, "submit + two reads — the poll stopped on FAILED");
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
        // The two-step contract on the money path: retargeted (checkpoint 2 item I) from the
        // retired `errors.yaml#/PriceMismatch` to `QuoteVerificationFailed` -- the structural code
        // the signed-quote verify guard now actually rejects with (D-D) -- arrives as REJECTED
        // with its errors.yaml code — normal UX flow (re-quote the cart), never an exception.
        let fake = FakeTransport::scripted(vec![
            Ok(acceptance("PENDING")),
            Ok(json!({ "operationStatus": {
                "messageId": "00000000-0000-7000-8000-000000000000",
                "correlationId": "00000000-0000-7000-8000-000000000000",
                "status": "REJECTED", "errorCode": "QuoteVerificationFailed",
                "message": "We couldn't confirm your total", "occurredAt": "2026-07-23T12:00:00Z",
            }})),
        ]);
        let placed = submit(&fake, &ctx(), &form(), "pm_123").await.unwrap();
        match placed.handle.resolve_with(&fake, 5, Duration::ZERO).await.unwrap() {
            ActionOutcome::Rejected { error_code, .. } => assert_eq!(error_code, "QuoteVerificationFailed"),
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

    /// The opening tag of the element with this id — so an attribute assertion (`disabled`) reads
    /// THE control it is about, not the whole page (the phone input is disabled by design).
    #[cfg(feature = "ssr")]
    fn element_tag<'a>(html: &'a str, id: &str) -> &'a str {
        let at = html.find(&format!("id=\"{id}\"")).expect("element exists");
        let open = html[..at].rfind('<').expect("tag start");
        let close = at + html[at..].find('>').expect("tag end");
        &html[open..=close]
    }

    /// The markup of the `data-s="<name>"` section -- so an assertion about the order summary
    /// reads THAT section, not a total that happens to appear elsewhere on the page (it is also on
    /// the pay button, which is the whole point of the other test).
    #[cfg(feature = "ssr")]
    fn section_at<'a>(html: &'a str, name: &str) -> &'a str {
        let at = html.find(&format!("data-s=\"{name}\"")).expect("section exists");
        let end = at + html[at..].find("</section>").expect("section end");
        &html[at..end]
    }

    #[cfg(feature = "ssr")]
    fn view_state(is_delivery: bool, payment_failed: bool) -> CheckoutViewState {
        CheckoutViewState {
            restaurant_name: "Chez Test".into(),
            cart_line_count: if is_delivery { 2 } else { 1 },
            formatted_total: "23,50 EUR".into(),
            is_delivery,
            payment_failed,
            locale: "fr".into(),
            // No key — the presence-gated default; tests that need the configured state attach
            // one via `with_publishable_key`.
            publishable_key: None,
            cart_failed: false,
            restaurant_failed: false,
            profile_failed: false,
            payment_status_failed: false,
        }
    }

    /// #424 quick win 2: the checkout page answers a refused payment with the spec's
    /// `payment_failed_state` — failure copy, the truthful "your cart is intact" promise (the saga
    /// places nothing and leaves the cart OPEN), and two client-kind `navigate` controls carrying
    /// the renderer's own DOM action contract. Absent unless the payment actually failed.
    #[cfg(feature = "ssr")]
    #[test]
    fn a_failed_payment_renders_the_failure_state_with_retry_and_an_intact_cart() {
        let html = render_checkout_html(view_state(true, true), "fr");
        assert!(html.contains("id=\"payment_failed_state\""), "the spec's section id: {html}");
        assert!(html.contains("data-c=\"conditional_section\""));
        // The copy resolves from the spec's translation keys, in the page's language.
        assert!(html.contains("Paiement refusé"), "fr title: {html}");
        assert!(html.contains("Votre carte n'a pas été débitée. Votre panier est intact."));
        assert!(html.contains("Réessayer le paiement"));
        assert!(html.contains("Revenir au panier"));
        assert!(
            !html.contains("[checkout.payment_failed"),
            "no `[key]` fallback marker — every key resolved against the generated catalog: {html}"
        );
        // Both controls carry the DOM contract the delegated listener navigates on — the markup is
        // ready; the listener is not installed for this screen yet (see the render comment, #420).
        assert!(html.contains("id=\"retry_payment_btn\""));
        assert!(html.contains("data-route=\"/checkout\""));
        assert!(html.contains("id=\"back_to_cart_btn\""));
        assert!(html.contains("data-route=\"/cart\""));

        // English keeps the same structure with the English copy.
        let en = render_checkout_html(view_state(true, true), "en");
        assert!(en.contains("Your card was not charged. Your cart is intact."), "{en}");

        // Not failed → the state does not exist at all (a spinner is bad; a false alarm is worse).
        let ok = render_checkout_html(view_state(true, false), "fr");
        assert!(!ok.contains("payment_failed_state"), "no failure state before a failure");
        assert!(!ok.contains("Paiement refusé"));
    }

    /// #420: the shell is built from the screen's DECLARED resolvers (through the render
    /// context's own binding resolution since #729 — data keyed by full resolver key). The
    /// end-to-end proof is
    /// `router::tests::the_checkout_shell_carries_the_cart_it_is_about_to_charge_for`; this pins
    /// the degradation cases that call site cannot easily reach — and the one that matters most
    /// is the LAST: an unresolvable payment status must never be read as a failure.
    #[test]
    fn the_shell_is_built_from_the_resolvers_and_degrades_without_lying() {
        let populated = |status: Value| {
            let mut ctx = crate::renderer::RenderContext::new("fr");
            ctx.insert_resolved(
                "cart.current",
                json!({
                    "lines": [{ "offerId": "o1" }, { "offerId": "o2" }, { "offerId": "o3" }],
                    "totalAmount": { "amountCents": 1990, "currency": "EUR" },
                    "serviceType": "COLLECTION",
                }),
            );
            ctx.insert_resolved("paymentStatus.byOrder", status);
            ctx
        };

        let state =
            CheckoutViewState::from_context(&populated(Value::Null), Some("chez-marco"), "fr");
        assert_eq!(state.cart_line_count, 3);
        assert_eq!(state.formatted_total, "19,90 EUR");
        assert_eq!(state.restaurant_name, "chez-marco", "the host's tenant label is the fallback");
        assert!(!state.is_delivery, "COLLECTION hides the delivery sections");
        assert!(!state.payment_failed);
        assert!(!state.cart_failed && !state.profile_failed && !state.payment_status_failed);

        // A name carried by the read WINS over the host fallback (forward-compatible with the
        // selection gap reported on #420).
        let mut named = populated(Value::Null);
        named.insert_resolved("cart.current", json!({ "restaurantName": "Chez Marco", "lines": [] }));
        let state = CheckoutViewState::from_context(&named, Some("chez-marco"), "fr");
        assert_eq!(state.restaurant_name, "Chez Marco");

        // FAILED reaches the flag; every other status — including a read that could not be
        // answered at all — leaves it false. Telling a customer their payment failed when we simply
        // do not know is the one mistake on this page that costs more than saying nothing.
        let failed = populated(json!({ "status": "FAILED" }));
        assert!(CheckoutViewState::from_context(&failed, None, "fr").payment_failed);
        for benign in [Value::Null, json!({ "status": "REQUIRES_CONFIRMATION" }), json!({})] {
            let state = CheckoutViewState::from_context(&populated(benign.clone()), None, "fr");
            assert!(!state.payment_failed, "{benign} must not read as a failure");
        }
        // Nothing resolved at all (an offline first paint): an empty shell, not a panic — and
        // NOT the failed state: unresolved ≠ failed (#730).
        let empty =
            CheckoutViewState::from_context(&crate::renderer::RenderContext::new("fr"), None, "fr");
        assert_eq!(empty.cart_line_count, 0);
        assert_eq!(empty.formatted_total, "");
        assert!(empty.is_delivery, "absent serviceType assumes the fuller DELIVERY form");
        assert!(!empty.payment_failed);
        assert!(!empty.cart_failed, "skip-by-design must never read as a failure");
    }

    /// #730: the failure flags come from the context's failure marks, per resolver — and an
    /// answer beats a failure mark (a failed re-read of an already-shown cart is answered).
    #[test]
    fn failure_marks_reach_the_view_state_per_resolver() {
        let mut ctx = crate::renderer::RenderContext::new("fr");
        ctx.insert_failed("cart.current");
        ctx.insert_failed("me.profile");
        ctx.insert_resolved("restaurant.bySlug", json!({ "displayName": "Chez Test" }));
        let state = CheckoutViewState::from_context(&ctx, None, "fr");
        assert!(state.cart_failed);
        assert!(state.profile_failed);
        assert!(!state.restaurant_failed, "restaurant ANSWERED — its failure flag stays off");
        assert!(!state.payment_status_failed, "paymentStatus was never read — unresolved ≠ failed");

        let mut ctx = crate::renderer::RenderContext::new("fr");
        ctx.insert_resolved("cart.current", json!({ "lines": [] }));
        ctx.insert_failed("cart.current");
        assert!(
            !CheckoutViewState::from_context(&ctx, None, "fr").cart_failed,
            "answer_beats_failure_mark"
        );
    }

    /// #730 (Tours-facing, the money page): a FAILED cart read renders the #472 error affordance
    /// in the order summary — never blank money — and `place_order_btn` disables with the
    /// translated reason (its `{{ cart.id }}` variable binds the failed read). The contact form
    /// and the rest of the page keep rendering.
    #[cfg(feature = "ssr")]
    #[test]
    fn a_failed_cart_read_errors_the_summary_and_disables_place_order() {
        let key = crate::stripe::PublishableKey::parse(Some("pk_test_abc123"));
        let mut state = view_state(true, false).with_publishable_key(key);
        state.cart_failed = true;
        state.formatted_total = String::new();
        let html = render_checkout_html(state, "fr");
        assert!(
            html.contains("<div data-c=\"cart_summary_mini\" data-error=\"true\""),
            "the summary renders the error affordance, element markup: {html}"
        );
        assert!(html.contains("Impossible de charger le contenu."), "{html}");
        assert!(html.contains("Réessayer"), "a user-initiated retry: {html}");
        assert!(
            element_tag(&html, "place_order_btn").contains("disabled"),
            "no order may be priced on data the customer never saw: {html}"
        );
        assert!(
            element_tag(&html, "place_order_btn")
                .contains("title=\"Impossible de charger le contenu.\""),
            "the disable carries its translated reason: {html}"
        );
        assert!(html.contains("id=\"contact_form\""), "the rest of the page keeps rendering: {html}");

        // A failed restaurant read alone: the summary stays (cart answered), the button disables
        // — `{{ restaurant.id }}` is a place_order variable — and the reason is VISIBLE at the
        // button (business lens, checkpoint: a `title` tooltip is invisible on mobile touch, our
        // primary surface), not a section-replacing error for a display-less read.
        let key = crate::stripe::PublishableKey::parse(Some("pk_test_abc123"));
        let mut state = view_state(true, false).with_publishable_key(key);
        state.restaurant_failed = true;
        let html = render_checkout_html(state, "fr");
        assert!(
            !html.contains("<div data-c=\"cart_summary_mini\" data-error=\"true\""),
            "no summary error for a display-less read: {html}"
        );
        assert!(html.contains("id=\"place_order_notice\""), "a VISIBLE reason at the button: {html}");
        assert!(html.contains("Impossible de charger le contenu."), "{html}");
        assert!(element_tag(&html, "place_order_btn").contains("disabled"), "{html}");
    }

    /// #730: a failed `me.profile` read leaves the contact fields EMPTY-EDITABLE (blocking manual
    /// entry would hide the working path) with a small notice — never a section-replacing error.
    #[cfg(feature = "ssr")]
    #[test]
    fn a_failed_profile_read_keeps_the_contact_fields_editable_with_a_notice() {
        let key = crate::stripe::PublishableKey::parse(Some("pk_test_abc123"));
        let mut state = view_state(true, false).with_publishable_key(key);
        state.profile_failed = true;
        let html = render_checkout_html(state, "fr");
        assert!(html.contains("id=\"contact_prefill_notice\""), "{html}");
        assert!(html.contains("id=\"contact_form\""), "the form still renders: {html}");
        assert!(
            !element_tag(&html, "full_name").contains("disabled"),
            "the manual path stays open: {html}"
        );
        assert!(
            !element_tag(&html, "place_order_btn").contains("disabled"),
            "a cosmetic prefill failure never blocks checkout (degraded-but-orderable): {html}"
        );
    }

    /// #730 checkpoint finding, pinned: a failed `paymentStatus.byOrder` read must NOT degrade
    /// the shell. Since #745 the paint loops SKIP that read entirely (declared structurally
    /// unfulfillable — `skipped_reads`), so production no longer sets this mark on first paint;
    /// the pin stays because the rule is about the GATE, not the frequency: whenever the mark IS
    /// set (a future arg-carrying read failing), `payment_unavailable_state` must not close
    /// checkout over it — the exact "cosmetic resolver failure blocks checkout" the business
    /// rule forbids. Caught by the server's own end-to-end body test
    /// (`hosts::tests::the_served_checkout_body_carries_the_key_iff_the_service_is_configured`)
    /// when a first draft did gate on it.
    #[cfg(feature = "ssr")]
    #[test]
    fn a_failed_payment_status_read_does_not_degrade_the_shell() {
        let key = crate::stripe::PublishableKey::parse(Some("pk_test_abc123"));
        let mut state = view_state(true, false).with_publishable_key(key);
        state.payment_status_failed = true;
        let html = render_checkout_html(state, "fr");
        assert!(!html.contains("id=\"payment_unavailable_state\""), "{html}");
        assert!(
            html.contains(&format!("id=\"{}\"", crate::stripe::MOUNT_ID)),
            "the element still mounts — the mark is structural on this route: {html}"
        );
        assert!(!element_tag(&html, "place_order_btn").contains("disabled"), "{html}");
    }

    /// The configured state (#440): a mountable key exists, so the spec tree INCLUDES the Stripe
    /// element. Before #440 this test ran on a key-less state and still passed its
    /// `data-c="stripe_express_checkout_element"` check — satisfied by the CSS SELECTOR in
    /// app.css inside `<style>`, not by any DOM — which is why the assertions below pin the
    /// mount div by `id=` + `data-pk=` (attribute syntax CSS text cannot fake).
    #[cfg(feature = "ssr")]
    #[test]
    fn checkout_renders_the_spec_component_tree() {
        let key = crate::stripe::PublishableKey::parse(Some("pk_test_abc123"));
        let html = render_checkout_html(view_state(true, false).with_publishable_key(key), "fr");
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
        assert!(
            html.contains(&format!("id=\"{}\"", crate::stripe::MOUNT_ID)),
            "the Stripe mount point must exist when a key is delivered: {html}"
        );
        // The delivery seam's last hop: the key rides the mount div as a data attribute, and the
        // stripe.js tag ships in THIS shell's head (checkout-only by decision).
        assert!(html.contains("data-pk=\"pk_test_abc123\""), "{html}");
        assert!(html.contains(crate::stripe::STRIPE_JS_TAG), "{html}");
        // The pay step says TEST MODE while the key is pk_test_ — persistent, translated.
        assert!(html.contains("id=\"test_mode_banner\""), "{html}");
        assert!(html.contains("Mode test — aucun débit réel"), "{html}");
        // No degrade state and an ENABLED pay button when payment can actually mount. (The
        // `disabled` check is scoped to the button's own tag: the phone input is disabled by
        // design, so a page-wide `contains` would always fire.)
        assert!(!html.contains("payment_unavailable_state"), "{html}");
        assert!(!element_tag(&html, "place_order_btn").contains("disabled"), "pay button must be live: {html}");
        // COLLECTION hides the delivery sections.
        let collection = render_checkout_html(view_state(false, false), "fr");
        assert!(!collection.contains("data-s=\"delivery_details\""));
    }

    /// #817 (C. conso. **L221-14**, transposing **CRD 2011/83 Art. 8(2)**): immediately before a
    /// distance order the button must carry an unambiguous mention of the OBLIGATION TO PAY, next
    /// to a legible total. The sanction is not a fine but that **the consumer is not bound**, so
    /// this is pinned in BOTH locales and on BOTH sides of the seam this `sdui: false` screen has:
    ///
    ///   * the CATALOG assertion goes red if `checkout.place_order` is reverted in
    ///     `specs/screens/restaurant_frontoffice.translations.yaml` to a price-only label;
    ///   * the RENDER assertion goes red if the button stops resolving that key -- the literal it
    ///     replaced (`"Place order - "{total}`) stated an amount AND was English on a French page.
    ///
    /// Whether this wording SATISFIES the article is counsel's call (QT-4 on the counsel list,
    /// `docs/legal/BRIEF-20260831-repricing-and-price-quote-counsel-packet.md`, and no counsel is
    /// engaged). The test asserts what the copy SAYS -- never that it complies, which this repo has
    /// no standing to assert (ADR-20260812-143619).
    #[cfg(feature = "ssr")]
    #[test]
    fn the_pay_button_states_the_obligation_to_pay_in_both_locales() {
        for (lang, obligation) in
            [("fr", "avec obligation de paiement"), ("en", "with obligation to pay")]
        {
            let catalog = crate::i18n::resolve("checkout.place_order", lang);
            assert!(
                catalog.contains(obligation),
                "the {lang} checkout.place_order copy must state the obligation to pay, not only \
                 the amount -- got {catalog:?}"
            );
            let state = view_state(true, false);
            let total = state.formatted_total.clone();
            let html = render_checkout_html(state, lang);
            let expected = crate::i18n::format_message(
                "checkout.place_order",
                lang,
                &[("total", total.as_str())],
            );
            assert!(
                html.contains(&expected),
                "the pay button must render the catalog label {expected:?} in {lang}: {html}"
            );
            // The total stays ON the button: the same article wants it legible at the moment of
            // ordering, so the obligation mention must not have displaced it.
            assert!(expected.contains(&total), "the label must keep the total: {expected:?}");
        }
    }

    /// #817, the other half of the same article: the recap of the total is CLEAR AND LEGIBLE only
    /// if the customer does not have to find and press something to see it. The screen declared the
    /// order-summary section `collapsible: true` and NO renderer ever read that flag -- so the flag
    /// was never the guarantee, and only an assertion on the rendered runtime can be.
    #[cfg(feature = "ssr")]
    #[test]
    fn the_total_recap_is_visible_before_ordering_without_expanding_anything() {
        let state = view_state(true, false);
        let total = state.formatted_total.clone();
        let html = render_checkout_html(state, "fr");
        let section = section_at(&html, "order_summary");
        assert!(
            section.contains(&total),
            "the payable total must be in the order-summary section's FIRST paint: {section}"
        );
        // And no collapse affordance ON THAT SECTION. Scoping matters: `aria-expanded` is the
        // REQUIRED ARIA attribute on any expandable control, so a page-wide scan would go red --
        // with a false diagnostic naming the recap -- the day someone adds a correct disclosure
        // widget elsewhere on the pay step. Punishing an accessibility fix is how a test gets
        // deleted rather than read (review round 1, #833).
        for affordance in ["data-collapsible", "data-collapsed", "aria-expanded"] {
            assert!(
                !section.contains(affordance),
                "{affordance} on the order-summary section would hide the recap: {section}"
            );
        }
        // `<details` stays PAGE-WIDE on purpose: a `<details>` opened before the section and closed
        // after it collapses the recap from the OUTSIDE, where a section-scoped check is blind.
        assert!(!html.contains("<details"), "a <details> wrapper would hide the recap: {html}");
    }

    /// #440, the designed red made permanent: the KEY-ABSENT render. This test's ancestor asserted
    /// "the Stripe mount point must exist" unconditionally and went red the moment the shell became
    /// presence-gated — this is that assertion's new life as the degrade contract. Absent key ⇒ NO
    /// element div, NO data-pk, NO stripe.js request (privacy: nothing leaves the browser), the
    /// spec's `payment_unavailable_state` with resolved copy, and a DISABLED pay button — never a
    /// dead control.
    #[cfg(feature = "ssr")]
    #[test]
    fn a_key_less_checkout_degrades_honestly_instead_of_rendering_a_dead_element() {
        // view_state() carries no key — exactly what production builds when the config is unset
        // (and, on the development profile, when it is set but malformed: parse maps both to None).
        let html = render_checkout_html(view_state(true, false), "fr");
        assert!(
            !html.contains(&format!("id=\"{}\"", crate::stripe::MOUNT_ID)),
            "no mount div may render when nothing can ever mount into it: {html}"
        );
        assert!(!html.contains("data-pk="), "{html}");
        assert!(
            !html.contains("js.stripe.com"),
            "no key ⇒ no stripe.js ⇒ no Stripe request from the customer's browser: {html}"
        );
        assert!(html.contains("id=\"payment_unavailable_state\""), "{html}");
        assert!(html.contains("data-c=\"conditional_section\""), "{html}");
        // The copy, resolved in the page's language — with the promise the system actually keeps.
        assert!(html.contains("Paiement momentanément indisponible"), "{html}");
        assert!(
            html.contains("votre panier est conservé, réessayez dans quelques minutes"),
            "{html}"
        );
        assert!(!html.contains("[checkout.payment_unavailable"), "no `[key]` fallback: {html}");
        // A way out (outline, the renderer's own navigate contract) + a pay button that cannot lie.
        assert!(html.contains("id=\"payment_unavailable_back_btn\""), "{html}");
        assert!(html.contains("data-route=\"/cart\""), "{html}");
        assert!(
            element_tag(&html, "place_order_btn").contains("disabled"),
            "place_order_btn must be disabled: {html}"
        );
        assert!(!html.contains("id=\"test_mode_banner\""), "no banner without a key: {html}");

        // English keeps the same structure.
        let en = render_checkout_html(view_state(true, false), "en");
        assert!(en.contains("Payment temporarily unavailable"), "{en}");
    }
}
