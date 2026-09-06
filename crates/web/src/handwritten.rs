//! The hand-written (`sdui: false`) screens, as a **type** rather than as a runtime string test
//! (#420, compiler-first — ADR-20260803-234035).
//!
//! Two screens legitimately opt out of the SDUI renderer: `checkout` (Stripe Elements + payment
//! security) and `order_tracking` (realtime subscription + order state machine). Both the SSR
//! entry (`router::render_matched`) and the hydrate entry (`renderer::hydrate`) used to dispatch on
//! `screen.sdui` with a `_ =>` arm underneath, and that silent default is exactly what shipped a
//! checkout page that mounted nothing and a confirmation page that rendered the not-found hero for
//! every order (PROP-20260809-021351 §2, G5/G6).
//!
//! The fix is not a test. [`HandWrittenScreen`] is a closed enum, every consumer matches it
//! EXHAUSTIVELY (no `_` arm exists anywhere), and two `const` blocks below prove — at COMPILE TIME,
//! against the generated screen tables — that the enum and the DSL's `sdui: false` set are the same
//! set. So:
//!
//!   * a new `sdui: false` screen in `specs/screens/**` FAILS THE BUILD until a variant + its mount
//!     exist (the mistake the old code made silently is now unspellable);
//!   * a variant whose screen id no longer exists FAILS THE BUILD too (no stale mounts);
//!   * adding a variant without wiring its render/mount fails the build on the exhaustive matches.
//!
//! The old `every_sdui_screen_of_every_surface_renders()` test skipped `!screen.sdui` screens — it
//! excluded precisely the two broken ones while its name claimed "every screen". The const proofs
//! below replace that hole with something a reviewer cannot forget to run.

use crate::generated::screens::{self, Screen};

/// One screen whose markup is hand-written rather than walked out of the generated tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandWrittenScreen {
    /// `restaurant_frontoffice.yaml#/screens/checkout` — `checkout.rs`.
    Checkout,
    /// `restaurant_frontoffice.yaml#/screens/order_tracking` — `tracking.rs`.
    OrderTracking,
    /// `restaurant_backoffice.yaml#/screens/sign_in_return` (#639 part C step 6-ii round 2,
    /// R2-D/R2-R2) — `sign_in_return.rs`. Query-string token extraction + the acceptance-first
    /// confirm/claim/route sequencing this DSL declares no binding grammar for, the SAME reasons
    /// `Checkout`/`OrderTracking` are hand-written.
    SignInReturn,
    /// `restaurant_backoffice.yaml#/screens/invitation_accept` (#639 part C step 6-iv round 2,
    /// ADR-20260905-101349 §2 amendment) — `invitation_accept.rs`. The `SignInReturn` shape,
    /// doubled: TWO commands sequenced client-side (never a process manager).
    InvitationAccept,
    /// `system.yaml#/screens/admin_sign_in_return` (#639 part C step 6-iii, ADR-20260906-023825) --
    /// `admin_sign_in_return.rs`. The `SignInReturn` System TWIN: same query-string/acceptance-
    /// first reasons, a DIFFERENT screen id owner (this variant, not `SignInReturn`) because the
    /// two live on different surfaces with different routes/actions/copy.
    AdminSignInReturn,
}

impl HandWrittenScreen {
    /// Every variant — the half of the correspondence the const proof walks forward.
    pub const ALL: &'static [HandWrittenScreen] = &[
        HandWrittenScreen::Checkout,
        HandWrittenScreen::OrderTracking,
        HandWrittenScreen::SignInReturn,
        HandWrittenScreen::InvitationAccept,
        HandWrittenScreen::AdminSignInReturn,
    ];

    /// The generated `Screen::id` this variant owns.
    pub const fn screen_id(self) -> &'static str {
        match self {
            HandWrittenScreen::Checkout => "checkout",
            HandWrittenScreen::OrderTracking => "order_tracking",
            HandWrittenScreen::SignInReturn => "sign_in_return",
            HandWrittenScreen::InvitationAccept => "invitation_accept",
            HandWrittenScreen::AdminSignInReturn => "admin_sign_in_return",
        }
    }

    /// The variant owning a screen id, if any. `const` so the proofs below can run it at compile
    /// time — which is the whole point.
    pub const fn from_screen_id(id: &str) -> Option<HandWrittenScreen> {
        let mut i = 0;
        while i < Self::ALL.len() {
            let candidate = Self::ALL[i];
            if str_eq(candidate.screen_id(), id) {
                return Some(candidate);
            }
            i += 1;
        }
        None
    }

    /// The variant serving a matched screen. `Some` for exactly the `sdui: false` screens — proved
    /// below, so a caller may treat `None` as "this is an SDUI screen", never as "unknown".
    pub const fn of(screen: &Screen) -> Option<HandWrittenScreen> {
        if screen.sdui {
            None
        } else {
            Self::from_screen_id(screen.id)
        }
    }
}

/// Byte-wise `&str` equality usable in `const` context (`==` on `str` is not const-callable).
const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Every generated surface table — the source of truth both proofs walk.
///
/// ALL FIVE generated modules, including `system`: the mob review of #427 planted an `sdui: false`
/// screen in `system` and it compiled clean, because this list had four entries while the proofs'
/// own doc promised "every `sdui: false` screen in the generated tables". A compiler-first gate
/// whose premise is a hand-kept list is only as good as the list. Harmless when it was found
/// (`Surface` has no `System` variant, so those screens are not host-routed) and latent the day
/// one is added — which is exactly the shape of defect these proofs exist to make impossible.
const SURFACES: [&[Screen]; 5] = [
    screens::captain_frontoffice::SCREENS,
    screens::restaurant_frontoffice::SCREENS,
    screens::restaurant_backoffice::SCREENS,
    screens::rider::SCREENS,
    screens::system::SCREENS,
];

// PROOF 1 (spec -> type): every `sdui: false` screen in the generated tables has a variant. Adding
// a hand-written screen to the DSL without wiring its mount is now a COMPILE ERROR, not a page that
// silently renders nothing.
const _: () = {
    let mut s = 0;
    while s < SURFACES.len() {
        let table = SURFACES[s];
        let mut i = 0;
        while i < table.len() {
            if !table[i].sdui {
                assert!(
                    HandWrittenScreen::from_screen_id(table[i].id).is_some(),
                    "a `sdui: false` screen has no HandWrittenScreen variant: add the variant and \
                     its render/mount arms (crates/web/src/handwritten.rs)"
                );
            }
            i += 1;
        }
        s += 1;
    }
};

// PROOF 2 (type -> spec): every variant still names a real `sdui: false` screen. A screen removed
// from the DSL, renamed, or flipped to `sdui: true` leaves a dead mount behind — also a compile
// error, so the two sets can never drift apart in EITHER direction.
const _: () = {
    let mut v = 0;
    while v < HandWrittenScreen::ALL.len() {
        let wanted = HandWrittenScreen::ALL[v].screen_id();
        let mut found = false;
        let mut s = 0;
        while s < SURFACES.len() {
            let table = SURFACES[s];
            let mut i = 0;
            while i < table.len() {
                if !table[i].sdui && str_eq(table[i].id, wanted) {
                    found = true;
                }
                i += 1;
            }
            s += 1;
        }
        assert!(
            found,
            "a HandWrittenScreen variant names no `sdui: false` screen in the generated tables: \
             the screen was removed, renamed, or is SDUI now (crates/web/src/handwritten.rs)"
        );
        v += 1;
    }
};

/// The SSR half: one hand-written screen → its full document, built from the resolvers the screen
/// DECLARES rather than from constants. Exhaustive by construction — there is no `_` arm, so a new
/// variant cannot reach production without a shell.
#[cfg(feature = "ssr")]
impl HandWrittenScreen {
    pub fn render_html(
        self,
        matched: &crate::router::RouteMatch,
        ctx: &crate::renderer::RenderContext,
        tenant: Option<&str>,
        locale: &str,
    ) -> String {
        match self {
            HandWrittenScreen::Checkout => crate::checkout::render_checkout_html(
                crate::checkout::CheckoutViewState::from_context(ctx, tenant, locale)
                    // The delivery seam (#440): the parsed key rides the RenderContext from the
                    // server's config into the shell; None = the degraded state.
                    .with_publishable_key(ctx.stripe_publishable_key.clone()),
                locale,
            ),
            HandWrittenScreen::OrderTracking => crate::tracking::render_tracking_html(
                // #472: from_context, so a FAILED order read renders the staleness reassurance
                // instead of the silent shell (and never the not-found hero).
                crate::tracking::TrackingState::from_context(order_id_of(matched), ctx),
                locale,
            ),
            // The token lives in the query string, which SSR never sees (`RouteMatch` strips it —
            // "query strings are the caller's to strip"): the shell is a STATIC working message,
            // and the real work happens in the browser (`mount`, below).
            HandWrittenScreen::SignInReturn => crate::sign_in_return::render_sign_in_return_html(locale),
            // The token/invitationId live in the query string, which SSR never sees: the shell is
            // a STATIC working message, and the real work happens in the browser (`mount`, below).
            HandWrittenScreen::InvitationAccept => {
                crate::invitation_accept::render_invitation_accept_html(locale)
            }
            // The token lives in the query string, which SSR never sees: the shell is a STATIC
            // working message, and the real work happens in the browser (`mount`, below).
            HandWrittenScreen::AdminSignInReturn => {
                crate::admin_sign_in_return::render_admin_sign_in_return_html(locale)
            }
        }
    }
}

/// The confirmation route's `:orderId`. A malformed id yields the nil UUID, which reads back from
/// the transport as "no such order" — the not-found hero, never a panic on a URL a stranger typed.
#[cfg(any(feature = "ssr", feature = "hydrate"))]
pub(crate) fn order_id_of(matched: &crate::router::RouteMatch) -> uuid::Uuid {
    matched
        .param("orderId")
        .and_then(|v| uuid::Uuid::parse_str(v).ok())
        .unwrap_or_else(uuid::Uuid::nil)
}

/// The BROWSER half: what `renderer::hydrate()` does when the matched screen is hand-written.
///
/// Until #420 it did nothing at all — `hydrate()` returned before the crate's only
/// `mount_to_body`, so in a real browser the checkout page mounted no component and the tracking
/// page stayed the static shell the server sent, forever (PROP-20260809-021351 §2, G5/G6/G7).
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub mod mount {
    use leptos::prelude::*;

    use super::HandWrittenScreen;
    use crate::graphql::{HttpTransport, Role};
    use crate::renderer::RenderContext;
    use crate::router::RouteMatch;
    use crate::session::SessionId;

    /// Mount the hand-written screen for a matched route. Exhaustive over the enum — a new variant
    /// fails to compile until it says what it does in a browser.
    /// `host` is the request Host (`chez-test.captain.food`) and `origin` the scheme-qualified
    /// origin (`https://chez-test.captain.food`) — two different strings that must not be swapped:
    /// `Surface::slug_of` splits on `:` to strip a port, so handing it an origin makes it read
    /// `https` and the storefront label silently disappears.
    #[allow(clippy::too_many_arguments)]
    pub fn mount(
        hand_written: HandWrittenScreen,
        matched: RouteMatch,
        host: String,
        origin: String,
        role: Role,
        session: SessionId,
        locale: String,
    ) {
        // The delegated action layer, exactly as the SDUI path installs it: checkout's
        // `payment_failed_state` renders two client-kind `navigate` buttons carrying the renderer's
        // own DOM contract, and without this listener they are controls that render and do nothing
        // — the failure mode CLAUDE.md calls worse than no control at all. `matched.screen` (#639
        // 4-ii): none of these hand-written screens declare `restricted:`/`unauthenticated:` today,
        // but the driver's bounce decision reads the SAME field the SDUI path does.
        crate::interact::install(&origin, role, session, matched.screen);
        let transport = HttpTransport::new(&origin, role, session);
        let tenant = crate::router::Surface::slug_of(&host).map(str::to_string);

        wasm_bindgen_futures::spawn_local(async move {
            let ctx = resolve_requirements(&transport, role, &matched, &locale).await;
            match hand_written {
                HandWrittenScreen::Checkout => mount_checkout(tenant, locale, ctx),
                HandWrittenScreen::OrderTracking => {
                    mount_tracking(matched, transport, origin, role, session, locale, ctx)
                }
                HandWrittenScreen::SignInReturn => {
                    mount_sign_in_return(transport, origin, session, locale)
                }
                HandWrittenScreen::InvitationAccept => {
                    mount_invitation_accept(transport, origin, session, locale)
                }
                HandWrittenScreen::AdminSignInReturn => {
                    mount_admin_sign_in_return(transport, origin, session, locale)
                }
            }
        });
    }

    /// Checkout in the browser (#440): read the publishable key back off the SSR shell's mount div
    /// (`data-pk` — the server wrote it there iff a mountable key was configured), re-render with
    /// live data, then attach the Stripe payment element.
    ///
    /// The element mounts in Stripe's DEFERRED posture (`mode: payment` + the cart's own total):
    /// acceptance-first means no PaymentIntent exists yet on the /checkout landing, and the intent
    /// created after PlaceOrder is what the confirm leg will pin by clientSecret. A mount FAILURE
    /// (stripe.js blocked/unloaded, Stripe threw) degrades to the SAME `payment_unavailable_state`
    /// the key-less shell renders — honestly, via the state signal — and is logged to the console
    /// only: no beacon exists (no OTel in WASM), which is why the contract's
    /// `stripe_js_load_failed`/`mount_threw` reasons are documented-for-future, not emitted.
    fn mount_checkout(tenant: Option<String>, locale: String, ctx: RenderContext) {
        let document = web_sys::window().and_then(|w| w.document());
        let publishable_key = crate::stripe::PublishableKey::parse(
            document
                .as_ref()
                .and_then(|d| d.get_element_by_id(crate::stripe::MOUNT_ID))
                .and_then(|el| el.get_attribute(crate::stripe::MOUNT_KEY_ATTR))
                .as_deref(),
        );
        // The deferred mount needs the cart's own total (display-level; the charge is always the
        // server-side intent). No resolved total ⇒ nothing to mount against — the element div
        // stays empty and the degrade state is NOT shown (the copy would lie: payment is not
        // "unavailable", the cart is empty/unresolved and the pay button leads nowhere anyway).
        let cart = ctx.binding_json("cart");
        let total = cart.as_ref().and_then(|c| c.get("totalAmount"));
        let amount_cents =
            total.and_then(|t| t.get("amountCents")).and_then(serde_json::Value::as_i64);
        let currency = total
            .and_then(|t| t.get("currency"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);

        let state = crate::checkout::CheckoutViewState::from_context(
            &ctx,
            tenant.as_deref(),
            &locale,
        )
        .with_publishable_key(publishable_key.clone());
        let state = RwSignal::new(state);
        leptos::mount::mount_to_body(move || {
            view! {
                {move || crate::checkout::CheckoutScreen(crate::checkout::CheckoutScreenProps {
                    state: state.get(),
                })}
            }
        });

        let Some(pk) = publishable_key else { return };
        let (Some(amount_cents), Some(currency)) = (amount_cents, currency) else {
            web_sys::console::warn_1(
                &"checkout: no resolved cart total — payment element not mounted".into(),
            );
            return;
        };
        if amount_cents <= 0 {
            return;
        }
        let config = crate::stripe::ElementsConfig::Deferred { amount_cents, currency };
        match crate::stripe::browser::PaymentElement::mount(pk.as_str(), &config) {
            // The mounted element lives in the DOM (a Stripe-hosted iframe); dropping OUR handle
            // does not unmount it. The confirm leg (a later #429 chunk) is what will need to hold
            // a PaymentElement across the submit flow.
            Ok(_element) => {}
            Err(e) => {
                web_sys::console::warn_1(
                    &format!("checkout: stripe.js mount failed — degrading honestly: {e}").into(),
                );
                state.update(|s| s.publishable_key = None);
            }
        }
    }

    /// The screen's DECLARED `data_requirements`, resolved through the browser's own transport —
    /// the same loop the SDUI path runs, so a hand-written screen is not a second data model.
    async fn resolve_requirements(
        transport: &HttpTransport,
        role: Role,
        matched: &RouteMatch,
        locale: &str,
    ) -> RenderContext {
        let mut ctx = RenderContext::new(locale);
        for resolver in matched.screen.data_requirements {
            // #745: the generated §25b skip table — a structurally unfulfillable read is skipped
            // before any network on this leg too (same authority as the SSR loop). The page's own
            // dispatch-time reads (checkout's paymentStatus poll with its client-minted orderId)
            // call `execute_resolver` directly and are untouched.
            if matched.screen.skipped_reads.contains(resolver) {
                continue;
            }
            let mut vars = serde_json::Map::new();
            for (k, v) in matched.param_args(*resolver) {
                vars.insert(k, v);
            }
            // #472: same classification as the SDUI paths — skip-by-design stays silent, a real
            // failure marks the binding failed (tracking's staleness state, checkout untouched).
            let result = crate::graphql::execute_resolver(transport, *resolver, vars).await;
            match crate::graphql::classify_resolve(role, *resolver, result) {
                crate::graphql::ResolveOutcome::Resolved(value) => {
                    ctx.insert_resolved(resolver.as_str(), value)
                }
                crate::graphql::ResolveOutcome::SkippedByDesign(_) => {}
                crate::graphql::ResolveOutcome::Failed(_) => ctx.insert_failed(resolver.as_str()),
            }
        }
        ctx
    }

    /// Tracking is pull-THEN-push over one state (`tracking.rs` module docs): mount what the pull
    /// gave us, then fold `orderStatusChanged` frames in and re-render. On every (re)connect the
    /// pull re-runs — the socket dies on instance restarts, so push is an accelerator and the
    /// re-sync is what makes a reconnect lossless.
    #[allow(clippy::too_many_arguments)]
    fn mount_tracking(
        matched: RouteMatch,
        transport: HttpTransport,
        origin: String,
        role: Role,
        session: SessionId,
        locale: String,
        ctx: RenderContext,
    ) {
        use std::rc::Rc;

        use crate::subscriptions::browser::{endpoint, Connection, Handle};
        use crate::subscriptions::SubscriptionKey;
        use crate::tracking::TrackingState;

        let order_id = super::order_id_of(&matched);
        // #758: the paid context — the browser still holds an OPEN PlaceOrder intent for this
        // order (`pending.rs`: written before the send, cleared only on a terminal outcome). It
        // licenses the "Reçu ✓ — confirmation en cours…" claim on an unresolved/answered-null
        // read, and arms the bounded birth re-check below.
        let birth_pending = crate::pending::holds_place_order(
            &crate::pending::BrowserPendingStore,
            order_id,
        );
        let state = RwSignal::new(
            TrackingState::from_context(order_id, &ctx).with_birth_pending(birth_pending),
        );
        {
            let locale = locale.clone();
            leptos::mount::mount_to_body(move || {
                let locale = locale.clone();
                view! {
                    {move || crate::tracking::OrderTrackingScreen(
                        crate::tracking::OrderTrackingScreenProps {
                            state: state.get(),
                            locale: locale.clone(),
                        },
                    )}
                }
            });
        }

        // A SECOND socket next to the one `interact::install` owns: the interaction driver holds its
        // handle privately and exposes no way for a screen to subscribe on it. Two sockets per
        // tracking page is a real peak cost and unifying them is a follow-up noted on #420 — but a
        // confirmation page that never updates is the worse of the two, and this is the honest fix
        // available without reshaping the interaction layer.
        let transport = Rc::new(transport);

        // #758: the BOUNDED birth re-check — paid context only, and only until the pull produces
        // the order. It stops on its own (`BIRTH_RECHECK_*`, the `await_payment_intent_with`
        // precedent — a convergence read, not a standing poll, ADR-20260810-231300); the
        // subscription below remains the push path and wins any race (an already-Present signal
        // is never overwritten by this leg).
        if birth_pending {
            let transport = Rc::clone(&transport);
            wasm_bindgen_futures::spawn_local(async move {
                let mut pulled = state.get_untracked();
                if matches!(pulled.order, crate::tracking::OrderRead::Present(_)) {
                    return;
                }
                let born = pulled
                    .load_until_present(
                        transport.as_ref(),
                        crate::tracking::BIRTH_RECHECK_MAX_ATTEMPTS,
                        crate::tracking::BIRTH_RECHECK_INTERVAL,
                    )
                    .await
                    .unwrap_or(false);
                if born
                    && !matches!(
                        state.get_untracked().order,
                        crate::tracking::OrderRead::Present(_)
                    )
                {
                    state.set(pulled);
                }
            });
        }

        let mut vars = serde_json::Map::new();
        vars.insert("orderId".into(), serde_json::json!(order_id));
        Connection::open(
            endpoint(&origin, role),
            // None is CORRECT for the browser, signed-in or not (#437): the customer's only
            // credential is the httpOnly `captain_auth` cookie (#112 — JS can never read it), and
            // the browser sends it on the same-origin WS upgrade automatically; the server falls
            // back to the upgrade headers when the init payload carries no token (`ws_auth_headers`).
            // `Some(...)` is for header-incapable-but-token-holding clients (e.g. desktop), never
            // the web storefront.
            None,
            session,
            Rc::new({
                let transport = Rc::clone(&transport);
                move |handle: &mut Handle| {
                    handle.subscribe(SubscriptionKey::OrderStatusChanged, vars.clone());
                    // Re-sync on EVERY (re)connect: the pull is the source of truth.
                    let transport = Rc::clone(&transport);
                    wasm_bindgen_futures::spawn_local(async move {
                        let mut pulled = state.get_untracked();
                        if pulled.load(transport.as_ref()).await.is_ok() {
                            state.set(pulled);
                        }
                    });
                }
            }),
            Rc::new(move |_sub, event| {
                let mut next = state.get_untracked();
                if next.apply(&event) {
                    state.set(next);
                }
            }),
        );
    }

    /// The magic-link RETURN landing (#639 part C step 6-ii round 2, R2-D/R2-R2): read `?token=`
    /// off the URL the mail client opened, dispatch `confirmMemberSignIn` (acceptance-first),
    /// poll its outcome, claim the parked session cookie (the `MemberNotLinked` leg parks one
    /// too, §8.5, so "Se déconnecter" has a real cookie there as well), then leave the page —
    /// a full navigation, not an SPA route change, since this page never mounts the router.
    fn mount_sign_in_return(transport: HttpTransport, origin: String, session: SessionId, locale: String) {
        use crate::sign_in_return::{SignInReturnScreen, SignInReturnScreenProps, SignInReturnState};

        let state = RwSignal::new(SignInReturnState::Working);
        {
            let locale = locale.clone();
            leptos::mount::mount_to_body(move || {
                let locale = locale.clone();
                view! {
                    {move || SignInReturnScreen(SignInReturnScreenProps { state: state.get(), locale: locale.clone() })}
                }
            });
        }

        let Some(token) = crate::sign_in_return::token_from_location() else {
            state.set(SignInReturnState::NoToken);
            return;
        };

        wasm_bindgen_futures::spawn_local(async move {
            let mut input = serde_json::Map::new();
            input.insert("token".into(), serde_json::json!(token));
            let outcome = async {
                let handle = crate::actions::dispatch(
                    &transport,
                    crate::generated::data_layer::ActionKey::ConfirmMemberSignIn,
                    input,
                )
                .await?;
                handle.resolve(&transport).await
            }
            .await;
            match outcome {
                Ok(crate::actions::ActionOutcome::Succeeded { message_id }) => {
                    crate::auth::claim_session(&origin, message_id, session).await;
                    crate::sign_in_return::navigate_away(&origin, "/");
                }
                Ok(crate::actions::ActionOutcome::Rejected { message_id, error_code, .. }) => {
                    // Best-effort: nothing was parked for every OTHER rejection (M5's shape), so
                    // this call is a genuine no-op there — never a reason to branch on it.
                    crate::auth::claim_session(&origin, message_id, session).await;
                    if error_code == "MemberNotLinked" {
                        crate::sign_in_return::navigate_away(&origin, "/sign-in/not-linked");
                    } else {
                        state.set(SignInReturnState::Failed);
                    }
                }
                Ok(crate::actions::ActionOutcome::Failed { .. }) | Err(_) => {
                    state.set(SignInReturnState::Failed);
                }
            }
        });
    }

    /// The ADMIN magic-link RETURN landing (#639 part C step 6-iii, ADR-20260906-023825): read
    /// `?token=` off the URL the mail client opened, dispatch `confirmAdminSignIn`
    /// (acceptance-first), poll its outcome, claim the parked session cookie (the
    /// `AdminAccessNotGranted` leg parks one too, so a future sign-out control would have a real
    /// cookie there as well), then leave the page — the `mount_sign_in_return` shape, transposed.
    fn mount_admin_sign_in_return(transport: HttpTransport, origin: String, session: SessionId, locale: String) {
        use crate::admin_sign_in_return::{AdminSignInReturnScreen, AdminSignInReturnScreenProps, AdminSignInReturnState};

        let state = RwSignal::new(AdminSignInReturnState::Working);
        {
            let locale = locale.clone();
            leptos::mount::mount_to_body(move || {
                let locale = locale.clone();
                view! {
                    {move || AdminSignInReturnScreen(AdminSignInReturnScreenProps { state: state.get(), locale: locale.clone() })}
                }
            });
        }

        let Some(token) = crate::admin_sign_in_return::token_from_location() else {
            state.set(AdminSignInReturnState::NoToken);
            return;
        };

        wasm_bindgen_futures::spawn_local(async move {
            let mut input = serde_json::Map::new();
            input.insert("token".into(), serde_json::json!(token));
            let outcome = async {
                let handle = crate::actions::dispatch(
                    &transport,
                    crate::generated::data_layer::ActionKey::ConfirmAdminSignIn,
                    input,
                )
                .await?;
                handle.resolve(&transport).await
            }
            .await;
            match outcome {
                Ok(crate::actions::ActionOutcome::Succeeded { message_id }) => {
                    crate::auth::claim_session(&origin, message_id, session).await;
                    crate::admin_sign_in_return::navigate_away(&origin, "/");
                }
                Ok(crate::actions::ActionOutcome::Rejected { message_id, error_code, .. }) => {
                    // Best-effort: nothing was parked for every OTHER rejection, so this call is a
                    // genuine no-op there — never a reason to branch on it.
                    crate::auth::claim_session(&origin, message_id, session).await;
                    if error_code == "AdminAccessNotGranted" {
                        crate::admin_sign_in_return::navigate_away(&origin, "/sign-in/no-access");
                    } else {
                        state.set(AdminSignInReturnState::Failed);
                    }
                }
                Ok(crate::actions::ActionOutcome::Failed { .. }) | Err(_) => {
                    state.set(AdminSignInReturnState::Failed);
                }
            }
        });
    }

    /// The invitation acceptance landing (#639 part C step 6-iv round 2, ADR-20260905-101349 §2
    /// amendment): read `?token=&invitationId=` off the URL the mail client opened, dispatch
    /// `acceptRestaurantInvitation` (leg 1), then — on its success — `grantRestaurantAccessByInvitation`
    /// (leg 2, retried up to `GRANT_LEG_MAX_ATTEMPTS` on a technical failure: business requires
    /// never showing "link no longer valid" to someone who already accepted), claim the parked
    /// session, then leave the page — a full navigation, the `mount_sign_in_return` shape.
    fn mount_invitation_accept(transport: HttpTransport, origin: String, session: SessionId, locale: String) {
        use crate::invitation_accept::{
            InvitationAcceptScreen, InvitationAcceptScreenProps, InvitationAcceptState, GRANT_LEG_MAX_ATTEMPTS,
        };

        let state = RwSignal::new(InvitationAcceptState::Working);
        {
            let locale = locale.clone();
            leptos::mount::mount_to_body(move || {
                let locale = locale.clone();
                view! {
                    {move || InvitationAcceptScreen(InvitationAcceptScreenProps { state: state.get(), locale: locale.clone() })}
                }
            });
        }

        let Some((token, invitation_id)) = crate::invitation_accept::params_from_location() else {
            state.set(InvitationAcceptState::NoToken);
            return;
        };

        wasm_bindgen_futures::spawn_local(async move {
            let accept_input = || {
                let mut m = serde_json::Map::new();
                m.insert("invitationId".into(), serde_json::json!(invitation_id));
                m.insert("token".into(), serde_json::json!(token));
                m
            };
            let leg1 = async {
                let handle = crate::actions::dispatch(
                    &transport,
                    crate::generated::data_layer::ActionKey::AcceptRestaurantInvitation,
                    accept_input(),
                )
                .await?;
                handle.resolve(&transport).await
            }
            .await;
            match leg1 {
                Ok(crate::actions::ActionOutcome::Succeeded { .. }) => {}
                // Leg 1 refused (unknown/wrong-email/already-accepted-by-someone-else/revoked/
                // expired — the server's own no-enumeration property, ONE typed refusal for all
                // five) or a technical failure: never worded differently, by design.
                _ => {
                    state.set(InvitationAcceptState::Failed);
                    return;
                }
            }

            // Leg 2: the SAME (invitationId, token) proves the caller IS the accepting subject.
            // Retried on a technical failure — never on a business rejection past the door
            // (`MemberAccessGrantDoorClosed`/`MemberAuthSubjectAlreadyBound`/
            // `RestaurantInvitationNotAcceptable`), which will not heal by retrying.
            let mut attempt = 0;
            loop {
                attempt += 1;
                let leg2 = async {
                    let handle = crate::actions::dispatch(
                        &transport,
                        crate::generated::data_layer::ActionKey::GrantRestaurantAccessByInvitation,
                        accept_input(),
                    )
                    .await?;
                    handle.resolve(&transport).await
                }
                .await;
                match leg2 {
                    Ok(crate::actions::ActionOutcome::Succeeded { message_id }) => {
                        crate::auth::claim_session(&origin, message_id, session).await;
                        crate::invitation_accept::navigate_away(&origin, "/");
                        return;
                    }
                    Ok(crate::actions::ActionOutcome::Rejected { .. }) => {
                        // A business refusal past the door never heals by retrying.
                        state.set(InvitationAcceptState::AccessPending);
                        return;
                    }
                    Ok(crate::actions::ActionOutcome::Failed { .. }) | Err(_) => {
                        if attempt >= GRANT_LEG_MAX_ATTEMPTS {
                            state.set(InvitationAcceptState::AccessPending);
                            return;
                        }
                        crate::actions::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::Surface;

    /// The const proofs above are the real gate — this only pins the mapping a reader would want to
    /// see spelled out, and fails loudly if `of()` ever stops agreeing with `sdui`.
    #[test]
    fn the_hand_written_set_is_exactly_the_non_sdui_set() {
        let mut seen = Vec::new();
        for surface in [
            Surface::CaptainFrontoffice,
            Surface::RestaurantFrontoffice,
            Surface::RestaurantBackoffice,
            Surface::Rider,
            Surface::System,
        ] {
            for screen in surface.screens() {
                match HandWrittenScreen::of(screen) {
                    Some(hw) => {
                        assert!(!screen.sdui, "{}: SDUI screen claimed by a mount", screen.id);
                        assert_eq!(hw.screen_id(), screen.id);
                        seen.push(hw);
                    }
                    None => assert!(screen.sdui, "{}: no mount for a hand-written screen", screen.id),
                }
            }
        }
        assert_eq!(seen, HandWrittenScreen::ALL, "every variant is reached by a real route");
    }
}
