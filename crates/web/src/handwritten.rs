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
}

impl HandWrittenScreen {
    /// Every variant — the half of the correspondence the const proof walks forward.
    pub const ALL: &'static [HandWrittenScreen] =
        &[HandWrittenScreen::Checkout, HandWrittenScreen::OrderTracking];

    /// The generated `Screen::id` this variant owns.
    pub const fn screen_id(self) -> &'static str {
        match self {
            HandWrittenScreen::Checkout => "checkout",
            HandWrittenScreen::OrderTracking => "order_tracking",
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
                crate::checkout::CheckoutViewState::from_resolved(&ctx.data, tenant, locale),
                locale,
            ),
            HandWrittenScreen::OrderTracking => crate::tracking::render_tracking_html(
                crate::tracking::TrackingState::from_resolved(
                    order_id_of(matched),
                    &ctx.data,
                ),
                locale,
            ),
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
        // — the failure mode CLAUDE.md calls worse than no control at all.
        crate::interact::install(&origin, role, session);
        let transport = HttpTransport::new(&origin, role, session);
        let tenant = crate::router::Surface::slug_of(&host).map(str::to_string);

        wasm_bindgen_futures::spawn_local(async move {
            let ctx = resolve_requirements(&transport, &matched, &locale).await;
            match hand_written {
                HandWrittenScreen::Checkout => {
                    let state = crate::checkout::CheckoutViewState::from_resolved(
                        &ctx.data,
                        tenant.as_deref(),
                        &locale,
                    );
                    leptos::mount::mount_to_body(move || {
                        crate::checkout::CheckoutScreen(crate::checkout::CheckoutScreenProps {
                            state: state.clone(),
                        })
                    });
                }
                HandWrittenScreen::OrderTracking => {
                    mount_tracking(matched, transport, origin, role, session, locale, ctx)
                }
            }
        });
    }

    /// The screen's DECLARED `data_requirements`, resolved through the browser's own transport —
    /// the same loop the SDUI path runs, so a hand-written screen is not a second data model.
    async fn resolve_requirements(
        transport: &HttpTransport,
        matched: &RouteMatch,
        locale: &str,
    ) -> RenderContext {
        let mut ctx = RenderContext::new(locale);
        for resolver in matched.screen.data_requirements {
            let mut vars = serde_json::Map::new();
            for (k, v) in matched.param_args(*resolver) {
                vars.insert(k, v);
            }
            if let Ok(value) = crate::graphql::execute_resolver(transport, *resolver, vars).await {
                ctx.insert_resolved(resolver.as_str(), value);
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
        let state = RwSignal::new(TrackingState::from_resolved(order_id, &ctx.data));
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
        let mut vars = serde_json::Map::new();
        vars.insert("orderId".into(), serde_json::json!(order_id));
        Connection::open(
            endpoint(&origin, role),
            None, // the customer bearer token lands with staff/customer sign-in; PUBLIC pushes today
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
