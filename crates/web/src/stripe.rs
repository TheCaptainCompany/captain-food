//! Stripe payment element interop (split 3/4 of #21) — the reason checkout is `sdui: false`.
//!
//! Contract boundaries, stated once:
//!
//!   * The client only ever holds the **`clientSecret`** (from `paymentStatus.byOrder` /
//!     `paymentStatusChanged`) and the publishable key — never a secret key, never card data (the
//!     element is a Stripe-hosted iframe; PAN/CVC never touch our DOM, our WASM, or our API).
//!   * The **capture verdict is server truth**: Stripe's webhook lands as the inbound
//!     `PaymentCaptured`/`PaymentFailed` fact and folds into the read models. What
//!     `confirm_payment` returns here only drives immediate UX (show the 3DS sheet, surface a
//!     decline) — the tracking screen resolves the REAL outcome from the platform's own reads.
//!
//! Split like the WS layer: a native-testable description of WHAT we ask of Stripe.js
//! ([`ElementsConfig`] / [`ConfirmParams`]), and a thin `hydrate`-only driver that hands it to the
//! real `Stripe.js` global. The js interop surface is 4 calls: `Stripe(pk)`,
//! `stripe.elements({clientSecret, ...})`, `elements.create('expressCheckout').mount(sel)`,
//! `stripe.confirmPayment({elements, ...})` — kept to the minimum the screen spec names
//! (`stripe_express_checkout_element`, `on_confirm: confirm_payment`).

/// The DOM id the checkout screen renders for the element (`checkout.rs` view) and the driver
/// mounts into — the one string both sides must agree on.
pub const MOUNT_ID: &str = "stripe-payment-element";

/// The data attribute the SSR shell writes the publishable key into on the mount div, and the
/// hydrate path reads back — the second half of the DOM contract next to [`MOUNT_ID`] (#440).
/// The `view!` macro needs the literal `data-pk`, so a test pins the two spellings together.
pub const MOUNT_KEY_ATTR: &str = "data-pk";

/// The stripe.js `<script>` tag, injected into the CHECKOUT shell only, and only when a usable
/// publishable key is present (#440 — checkout-only over every-page: privacy over Stripe's
/// fraud-signal preference; the Stripe-cookie consent line is #400's). Stripe requires their
/// hosted bundle — no npm/wasm packaging of it exists by policy.
pub const STRIPE_JS_TAG: &str = "<script src=\"https://js.stripe.com/v3/\"></script>";

/// A publishable key the shell may actually hand to `Stripe(pk)`: trimmed, `pk_test_`-prefixed,
/// alphanumeric tail — the `scalars.yaml#/StripePublishableKeyTest` anchor re-applied at the LAST
/// hop. The ONLY constructor maps absent, empty and malformed all to `None` (#440 architect pin:
/// the generated config validation passes `Some("")` through, and the development profile
/// CONTINUES past an INVALID value — so "present in config" is not "mountable", and this type is
/// what makes the difference unspellable downstream: a degraded shell cannot be asked to mount,
/// and a `pk_live_` / `sk_*` / garbage value cannot reach stripe.js). At go-live the LIVE key gets
/// its own scalar and mode witness (ADR-20260809-050000 decision 5) — a sibling type then, never a
/// loosening here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishableKey(String);

impl PublishableKey {
    /// Parse a raw configured value; anything not mountable is `None` (= render the degraded shell).
    pub fn parse(raw: Option<&str>) -> Option<Self> {
        let v = raw?.trim();
        let tail = v.strip_prefix("pk_test_")?;
        (!tail.is_empty() && tail.bytes().all(|b| b.is_ascii_alphanumeric()))
            .then(|| Self(v.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this key is a TEST-mode key — drives the checkout pay-step banner ("Mode test —
    /// aucun débit réel"). Structurally always true today (the parser only admits `pk_test_`);
    /// kept as a METHOD so the banner follows the KEY when the live sibling type arrives, rather
    /// than a constant someone must remember to flip.
    pub fn is_test_mode(&self) -> bool {
        self.0.starts_with("pk_test_")
    }
}

/// What we configure `stripe.elements(...)` with — the two mount postures the flow needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementsConfig {
    /// `elements({clientSecret})`: pinned to THE PaymentIntent the PlaceOrderProcess created —
    /// amount/currency live server-side on the intent, so the client cannot even express a
    /// different charge.
    ForIntent { client_secret: String },
    /// `elements({mode: 'payment', amount, currency})`: Stripe's documented DEFERRED-intent
    /// posture — the element mounts BEFORE any intent exists, which is the /checkout landing
    /// state by construction (acceptance-first: the intent is only created after PlaceOrder).
    /// Amount/currency drive the element's wallet DISPLAY only; the charge is always the
    /// server-side intent, whose clientSecret pins the confirm leg.
    Deferred { amount_cents: i64, currency: String },
}

/// What `stripe.confirmPayment` is called with. `return_url` is where Stripe lands redirect-based
/// methods (3DS challenges, wallets) — the confirmation route, which re-resolves everything by
/// `orderId` (the whole flow is reload-proof by construction, #12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmParams {
    pub return_url: String,
}

/// The immediate (UX-only) result of a confirm call — see the module docs for why this is NOT the
/// payment verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmUx {
    /// Stripe accepted the confirmation (or took over via redirect) — show "processing" and let
    /// the platform reads deliver the verdict.
    Submitted,
    /// Stripe reported an immediate problem (validation, decline surfaced synchronously). The
    /// message is Stripe's localized user-facing one.
    Declined(String),
}

/// The `hydrate`-only driver over the `Stripe.js` browser global (loaded from a `<script>` tag —
/// Stripe requires their hosted bundle; no npm/wasm packaging of it exists by policy).
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub mod browser {
    use super::*;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        /// The `Stripe` constructor from https://js.stripe.com/v3/.
        #[wasm_bindgen(js_name = Stripe, catch)]
        fn stripe_js(publishable_key: &str) -> Result<JsStripe, JsValue>;

        type JsStripe;
        #[wasm_bindgen(method, catch)]
        fn elements(this: &JsStripe, options: &JsValue) -> Result<JsElements, JsValue>;
        #[wasm_bindgen(method, js_name = confirmPayment)]
        fn confirm_payment(this: &JsStripe, options: &JsValue) -> js_sys::Promise;

        type JsElements;
        #[wasm_bindgen(method, catch)]
        fn create(this: &JsElements, element_type: &str) -> Result<JsElement, JsValue>;

        type JsElement;
        #[wasm_bindgen(method, catch)]
        fn mount(this: &JsElement, selector: &str) -> Result<(), JsValue>;
    }

    /// A mounted payment element, ready to confirm.
    pub struct PaymentElement {
        stripe: JsStripe,
        elements: JsElements,
    }

    /// Everything the driver can fail with — all of it is "Stripe.js said no", stringified for the
    /// error surface (the user-facing message channel is [`ConfirmUx::Declined`], not this).
    #[derive(Debug, thiserror::Error)]
    #[error("stripe.js: {0}")]
    pub struct StripeJsError(String);

    fn js_err(v: JsValue) -> StripeJsError {
        StripeJsError(v.as_string().unwrap_or_else(|| format!("{v:?}")))
    }

    impl PaymentElement {
        /// `Stripe(pk)` + `elements({…})` + `create('expressCheckout').mount('#…')` — the element
        /// renders Stripe's wallet/card UI inside [`super::MOUNT_ID`]. The options object follows
        /// [`ElementsConfig`]: `clientSecret` when an intent exists, the deferred
        /// `mode/amount/currency` triple when checkout is still pre-order.
        pub fn mount(publishable_key: &str, config: &ElementsConfig) -> Result<Self, StripeJsError> {
            let stripe = stripe_js(publishable_key).map_err(js_err)?;
            let options = js_sys::Object::new();
            let set = |k: &str, v: JsValue| {
                js_sys::Reflect::set(&options, &k.into(), &v).map_err(js_err).map(|_| ())
            };
            match config {
                ElementsConfig::ForIntent { client_secret } => {
                    set("clientSecret", client_secret.as_str().into())?;
                }
                ElementsConfig::Deferred { amount_cents, currency } => {
                    set("mode", "payment".into())?;
                    set("amount", JsValue::from_f64(*amount_cents as f64))?;
                    // Stripe wants the lowercase ISO code ("eur"), the domain speaks "EUR".
                    set("currency", currency.to_lowercase().into())?;
                }
            }
            let elements = stripe.elements(&options).map_err(js_err)?;
            let element = elements.create("expressCheckout").map_err(js_err)?;
            element.mount(&format!("#{}", super::MOUNT_ID)).map_err(js_err)?;
            Ok(Self { stripe, elements })
        }

        /// `stripe.confirmPayment({elements, confirmParams:{return_url}})`. Resolves to the
        /// UX-only verdict — an `error` in the resolution is a synchronous decline; a clean
        /// resolution (or a redirect that never resolves) is `Submitted`.
        pub async fn confirm(&self, params: &ConfirmParams) -> Result<ConfirmUx, StripeJsError> {
            let confirm_params = js_sys::Object::new();
            js_sys::Reflect::set(
                &confirm_params,
                &"return_url".into(),
                &params.return_url.as_str().into(),
            )
            .map_err(js_err)?;
            let options = js_sys::Object::new();
            js_sys::Reflect::set(&options, &"elements".into(), self.elements.as_ref())
                .map_err(js_err)?;
            js_sys::Reflect::set(&options, &"confirmParams".into(), &confirm_params)
                .map_err(js_err)?;

            let resolved =
                wasm_bindgen_futures::JsFuture::from(self.stripe.confirm_payment(&options))
                    .await
                    .map_err(js_err)?;
            // Stripe resolves `{error}` on synchronous failure; anything else means submitted
            // (redirect flows navigate away before resolving at all).
            let decline = js_sys::Reflect::get(&resolved, &"error".into())
                .ok()
                .filter(|e| !e.is_undefined() && !e.is_null())
                .and_then(|e| {
                    js_sys::Reflect::get(&e, &"message".into()).ok().and_then(|m| m.as_string())
                });
            Ok(match decline {
                Some(message) => ConfirmUx::Declined(message),
                None => ConfirmUx::Submitted,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mount_id_is_a_stable_dom_contract() {
        // checkout.rs renders this id; the driver mounts `#<id>`. A rename must break a test.
        assert_eq!(MOUNT_ID, "stripe-payment-element");
        // checkout.rs's view! writes the literal `data-pk`; the hydrate read uses the const. The
        // two spellings must never drift (#440).
        assert_eq!(MOUNT_KEY_ATTR, "data-pk");
    }

    /// #440 red-first (architect pin + beck's invalid-as-absent case): "present in config" is not
    /// "mountable". The only constructor maps every unusable shape to `None` — including the
    /// empty string the generated validation passes through as `Some("")`, and the invalid values
    /// the DEVELOPMENT profile continues past — so the type makes "mount with a bad key"
    /// unspellable, not merely unlikely.
    #[test]
    fn only_a_real_pk_test_key_is_mountable_everything_else_is_absent() {
        assert!(PublishableKey::parse(Some("pk_test_abc123")).is_some());
        // Whitespace from a dashboard copy-paste is trimmed, not fatal.
        let k = PublishableKey::parse(Some("  pk_test_abc123  ")).expect("trimmed key parses");
        assert_eq!(k.as_str(), "pk_test_abc123");
        assert!(k.is_test_mode());

        for unusable in [
            None,                    // key never configured
            Some(""),                // architect pin: generated validation passes Some("")
            Some("   "),             // whitespace-only
            Some("pk_test_"),        // prefix with no tail
            Some("pk_live_abc123"),  // LIVE key in the test slot — must degrade, never mount
            Some("sk_test_abc123"),  // a SECRET key pasted where a browser value goes
            Some("pk_test_abc 123"), // corrupted paste
        ] {
            assert!(
                PublishableKey::parse(unusable).is_none(),
                "{unusable:?} must be absent (degrade), never a mountable value"
            );
        }
    }
}
