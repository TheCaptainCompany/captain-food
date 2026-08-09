//! Order confirmation & tracking (split 3/4 of #21) —
//! `restaurant_frontoffice.yaml#/screens/order_tracking`, deliberately NOT SDUI (`sdui: false`:
//! "Realtime GraphQL subscription + order state machine").
//!
//! The screen's data model is **pull-then-push over one state** ([`TrackingState`]):
//!
//!   * [`TrackingState::load`] resolves `order.byId` — the source of truth, re-run on every
//!     (re)connect (the subscription contract: the free-tier socket dies on restarts, so push is an
//!     accelerator and the pull re-sync is what makes reconnects lossless).
//!   * [`TrackingState::apply`] folds `orderStatusChanged` events in. The server re-resolves and
//!     pushes the FULL `Order` per event (same generated selection as the pull — enforced by
//!     `SubscriptionKey::selection` reusing it), so applying is REPLACE, not patch — there is no
//!     partial-merge state to get wrong. A stale frame (out-of-order delivery) is guarded by
//!     `statusChangedAt`: never replace newer with older.
//!
//! The status hero + post-delivery actions follow the spec's `status_config` /
//! `post_order_actions`: rating, delivery-satisfaction (#62) and tip all dispatch through the
//! acceptance-first layer like every other write.

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::actions::{dispatch, ActionError, DispatchHandle};
use crate::generated::data_layer::{ActionKey, ResolverKey};
use crate::graphql::{execute_resolver, ResolverError, Transport};
use crate::subscriptions::SubscriptionEvent;

/// The tracked order — one screen, one order, keyed by the route's `orderId` (#14: the subscription
/// takes exactly what the confirmation route holds).
#[derive(Debug, Clone, PartialEq)]
pub struct TrackingState {
    pub order_id: Uuid,
    /// The current `Order` subtree (`order.byId` shape). `None` until the first successful load —
    /// or when the caller may not see this order (the read resolves null to strangers; the UI
    /// renders the not-found state, never an existence oracle).
    pub order: Option<Value>,
}

impl TrackingState {
    pub fn new(order_id: Uuid) -> Self {
        Self { order_id, order: None }
    }

    /// Build from an already-RESOLVED render context — the screen's own declared `order.byId`
    /// requirement, resolved by whichever entry is rendering (SSR's `render_path_with`, or
    /// `hydrate`'s fetch loop). Before #420 the only production call site built
    /// `TrackingState::new(order_id)` unconditionally, so every confirmation page rendered the
    /// not-found hero regardless of what the order was actually doing (PROP-20260809-021351 §2, G6).
    ///
    /// A null/absent read yields `order: None` — the honest not-found state, never a guess.
    pub fn from_resolved(order_id: Uuid, data: &Map<String, Value>) -> Self {
        let order = data.get("order").filter(|v| !v.is_null()).cloned();
        Self { order_id, order }
    }

    /// Pull `order.byId` — the initial render AND the re-sync on every subscription (re)connect.
    pub async fn load(&mut self, transport: &dyn Transport) -> Result<(), ResolverError> {
        let mut vars = Map::new();
        vars.insert("id".into(), json!(self.order_id));
        let order = execute_resolver(transport, ResolverKey::OrderById, vars).await?;
        // A null read after a non-null one keeps the last known state: blipping the screen back
        // to "not found" mid-tracking would be a worse lie than a briefly-stale order.
        if !order.is_null() {
            self.order = Some(order);
        }
        Ok(())
    }

    /// Fold one subscription event in. Returns `true` when the state changed (the screen's
    /// re-render trigger). `Failed`/`Complete` never mutate — the driver's reconnect + `load`
    /// re-sync own recovery.
    pub fn apply(&mut self, event: &SubscriptionEvent) -> bool {
        let SubscriptionEvent::Next(pushed) = event else { return false };
        if pushed.is_null() {
            return false;
        }
        // Out-of-order guard: `statusChangedAt` is ISO-8601 UTC, so string order IS time order.
        // Frames without the field (or a first frame) always apply — replace semantics.
        let newer = match (self.timestamp(), timestamp_of(pushed)) {
            (Some(current), Some(incoming)) => incoming.as_str() >= current.as_str(),
            _ => true,
        };
        if newer {
            self.order = Some(pushed.clone());
        }
        newer
    }

    /// The current order's `status` token (`scalars.yaml#/OrderStatus`), when loaded.
    pub fn status(&self) -> Option<&str> {
        self.order.as_ref().and_then(|o| o.get("status")).and_then(Value::as_str)
    }

    fn timestamp(&self) -> Option<String> {
        self.order.as_ref().and_then(|o| timestamp_of(o))
    }
}

fn timestamp_of(order: &Value) -> Option<String> {
    order.get("statusChangedAt").and_then(Value::as_str).map(str::to_string)
}

/// The status hero content for one `OrderStatus` — the spec's `status_config` table (icons are its
/// literal tokens; titles/bodies are translation keys resolved by the i18n layer, split 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusHero {
    pub icon: &'static str,
    pub title_key: &'static str,
    pub body_key: &'static str,
}

/// `status_config` as data. Both CANCELLED_* statuses render the spec's single `CANCELLED` entry —
/// who cancelled changes the copy server-side (the pushed order carries it), not the hero shape.
pub fn status_hero(status: &str) -> Option<StatusHero> {
    let hero = |icon, key: &'static str| StatusHero {
        icon,
        title_key: key,
        body_key: match key {
            "order.status.placed.title" => "order.status.placed.body",
            "order.status.accepted.title" => "order.status.accepted.body",
            "order.status.rejected.title" => "order.status.rejected.body",
            "order.status.preparing.title" => "order.status.preparing.body",
            "order.status.ready.title" => "order.status.ready.body",
            "order.status.out_for_delivery.title" => "order.status.out_for_delivery.body",
            "order.status.delivered.title" => "order.status.delivered.body",
            _ => "order.status.cancelled.body",
        },
    };
    match status {
        "PLACED" => Some(hero("check_circle", "order.status.placed.title")),
        "ACCEPTED" => Some(hero("chef_hat", "order.status.accepted.title")),
        "REJECTED" => Some(hero("x_circle", "order.status.rejected.title")),
        "PREPARING" => Some(hero("fire", "order.status.preparing.title")),
        "READY" => Some(hero("bag_check", "order.status.ready.title")),
        "OUT_FOR_DELIVERY" => Some(hero("truck", "order.status.out_for_delivery.title")),
        "DELIVERED" => Some(hero("home", "order.status.delivered.title")),
        "CANCELLED_BY_CUSTOMER" | "CANCELLED_BY_RESTAURANT" => {
            Some(hero("x_circle", "order.status.cancelled.title"))
        }
        _ => None,
    }
}

// ─── Post-delivery actions (the `rating_sheet`, #62) ───────────────────────────────────────────

/// `rate_order` — the rider thumbs up/down (`commands.yaml#/RateOrder`; DELIVERED-only
/// server-side — the sheet only shows then). `rider_thumb` is a `ThumbRating` token (`UP`/`DOWN`).
pub async fn rate_order(
    transport: &dyn Transport,
    order_id: Uuid,
    restaurant_id: &str,
    rider_thumb: &str,
) -> Result<DispatchHandle, ActionError> {
    let mut input = Map::new();
    input.insert("orderId".into(), json!(order_id));
    input.insert("restaurantId".into(), json!(restaurant_id));
    input.insert("riderThumb".into(), json!(rider_thumb));
    dispatch(transport, ActionKey::RateOrder, input).await
}

/// `record_delivery_satisfaction` — the one-question timeliness survey (#62,
/// `commands.yaml#/RecordDeliverySatisfaction`). `reason` only accompanies a dissatisfied verdict
/// per the sheet spec.
pub async fn record_delivery_satisfaction(
    transport: &dyn Transport,
    order_id: Uuid,
    restaurant_id: &str,
    timeliness: &str,
    reason: Option<&str>,
) -> Result<DispatchHandle, ActionError> {
    let mut input = Map::new();
    input.insert("orderId".into(), json!(order_id));
    input.insert("restaurantId".into(), json!(restaurant_id));
    input.insert("timeliness".into(), json!(timeliness));
    if let Some(reason) = reason {
        input.insert("reason".into(), json!(reason));
    }
    dispatch(transport, ActionKey::RecordDeliverySatisfaction, input).await
}

/// One tip line for [`tip_order`] (`entities.yaml#/Tip`): recipient ROLE + amount. Additive
/// server-side (ADR-012) — multiple tips accumulate, Captain keeps 0%.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipLine {
    /// `scalars.yaml#/TipRecipient` token (RIDER, or RESTAURANT on self-dispatch).
    pub recipient: String,
    pub amount_cents: i64,
    pub currency: String,
}

/// `tip_order` — the post-delivery tip prompt (#62 reusing ADR-012's `commands.yaml#/TipOrder`:
/// `tips` is an array of Tip lines, one per recipient).
pub async fn tip_order(
    transport: &dyn Transport,
    order_id: Uuid,
    restaurant_id: &str,
    tips: &[TipLine],
) -> Result<DispatchHandle, ActionError> {
    let mut input = Map::new();
    input.insert("orderId".into(), json!(order_id));
    input.insert("restaurantId".into(), json!(restaurant_id));
    input.insert(
        "tips".into(),
        json!(tips
            .iter()
            .map(|t| json!({
                "recipient": t.recipient,
                "amount": { "amountCents": t.amount_cents, "currency": t.currency },
            }))
            .collect::<Vec<_>>()),
    );
    dispatch(transport, ActionKey::TipOrder, input).await
}

// ─── The tracking screen (Leptos, SSR + hydrate from the same tree) ────────────────────────────

use leptos::prelude::*;

/// The status copy of one hero, resolved into the page's language.
///
/// The title always renders. The BODY only renders when every `{param}` the catalog entry declares
/// could be filled: `order.status.accepted.body` is *"{restaurant} prépare votre commande."* and
/// `order.status.preparing.body` needs `{minutes}` — the `order.byId` selection carries neither the
/// restaurant's display name nor a countdown, so an un-interpolated `{restaurant}` would be shipped
/// copy that reads like a bug to the one customer most likely to be anxious. A title-only hero is
/// complete and true; a placeholder is neither. (The gap is a selection/DSL matter, reported on
/// #420 rather than papered over here.)
fn hero_copy(hero: &StatusHero, locale: &str) -> (String, Option<String>) {
    let title = crate::i18n::resolve(hero.title_key, locale);
    let body = crate::i18n::resolve(hero.body_key, locale);
    let body = (!body.contains('{')).then_some(body);
    (title, body)
}

/// The order tracking screen, rendered from the [`TrackingState`] fold — the spec's tree
/// (`order_status_hero`, `eta_bar`, `order_items_summary`, `order_id_row`, `post_order_actions`)
/// with the renderer's `data-c` tagging. The same tree serves SSR and hydrate.
///
/// `locale` is a prop rather than a field of [`TrackingState`] because the state is what the
/// transport says about the ORDER; the language is what the request says about the READER. Since
/// #420 the hero renders the resolved SENTENCE (it used to emit `data-i18n` attributes on EMPTY
/// elements — nothing anywhere turned those into words, so the page a customer landed on after
/// paying was literally blank above the fold).
#[component]
pub fn OrderTrackingScreen(state: TrackingState, locale: String) -> impl IntoView {
    let order_id = state.order_id.to_string();
    let status = state.status().map(str::to_string);
    let hero = status.as_deref().and_then(status_hero);
    let eta = state
        .order
        .as_ref()
        .and_then(|o| o.get("estimatedReadyAt"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let show_eta = matches!(status.as_deref(), Some("ACCEPTED" | "PREPARING" | "OUT_FOR_DELIVERY"));
    let delivered = status.as_deref() == Some("DELIVERED");
    let item_count = state
        .order
        .as_ref()
        .and_then(|o| o.get("items"))
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);

    let not_found = crate::i18n::resolve("order.not_found", &locale);
    view! {
        <main id="app" data-hydrate="order_tracking">
            {match hero {
                Some(h) => {
                    let (title, body) = hero_copy(&h, &locale);
                    view! {
                        <section data-c="order_status_hero" data-icon=h.icon data-status=status.clone().unwrap_or_default()>
                            <h1 data-i18n=h.title_key>{title}</h1>
                            {body.map(|body| view! { <p data-i18n=h.body_key>{body}</p> })}
                        </section>
                    }.into_any()
                }
                // No order (loading / stranger / unknown id): the not-found state — same DOM slot.
                None => view! {
                    <section data-c="order_status_hero" data-status="UNKNOWN">
                        <h1 data-i18n="order.not_found">{not_found}</h1>
                    </section>
                }.into_any(),
            }}
            {(show_eta && eta.is_some()).then(|| view! {
                <div data-c="eta_bar">{eta.clone().unwrap_or_default()}</div>
            })}
            <div data-c="order_timeline"></div>
            <div data-c="order_items_summary" data-count=item_count.to_string()></div>
            <div data-c="order_id_row">{order_id}</div>
            <section data-c="section" id="post_order_actions">
                {delivered.then(|| view! {
                    <button data-c="button" data-sheet="rating_sheet" data-i18n="order.rate"></button>
                })}
            </section>
        </main>
    }
}

/// Server-side render the tracking page (the `ssr` build).
#[cfg(feature = "ssr")]
pub fn render_tracking_html(state: TrackingState, lang: &str) -> String {
    let lang = crate::i18n::normalize_locale(lang).unwrap_or(crate::i18n::DEFAULT_LOCALE);
    // ONE source of truth for the language: the document's `lang` is also what the copy resolves in.
    let body =
        OrderTrackingScreen(OrderTrackingScreenProps { state, locale: lang.to_string() }).to_html();
    crate::renderer::page_html("Your order - Captain.Food", lang, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql::test_support::FakeTransport;

    fn order(status: &str, changed_at: &str) -> Value {
        json!({
            "id": "order-1", "status": status, "statusChangedAt": changed_at,
            "estimatedReadyAt": "2026-07-23T12:45:00Z",
            "items": [{ "offerId": "o1" }, { "offerId": "o2" }],
        })
    }

    #[tokio::test]
    async fn load_pulls_order_by_id_and_keeps_last_known_on_null() {
        let id = Uuid::now_v7();
        let fake = FakeTransport::scripted(vec![
            Ok(json!({ "order": order("PLACED", "2026-07-23T12:00:00Z") })),
            Ok(json!({ "order": null })),
        ]);
        let mut state = TrackingState::new(id);
        state.load(&fake).await.unwrap();
        assert_eq!(state.status(), Some("PLACED"));
        assert_eq!(fake.call(0).1["input"]["id"], json!(id));

        // A later null read (transient authz blip / replica lag) must NOT blank the screen.
        state.load(&fake).await.unwrap();
        assert_eq!(state.status(), Some("PLACED"), "null re-read keeps last known state");
    }

    #[test]
    fn apply_replaces_on_newer_and_drops_stale_frames() {
        let mut state = TrackingState::new(Uuid::now_v7());
        assert!(state.apply(&SubscriptionEvent::Next(order("PLACED", "2026-07-23T12:00:00Z"))));
        assert!(state.apply(&SubscriptionEvent::Next(order("ACCEPTED", "2026-07-23T12:05:00Z"))));
        assert_eq!(state.status(), Some("ACCEPTED"));

        // An out-of-order PLACED frame arriving late must not regress the screen.
        assert!(!state.apply(&SubscriptionEvent::Next(order("PLACED", "2026-07-23T12:00:00Z"))));
        assert_eq!(state.status(), Some("ACCEPTED"));

        // Failed/Complete never mutate — recovery is reconnect + re-load.
        assert!(!state.apply(&SubscriptionEvent::Failed("boom".into())));
        assert!(!state.apply(&SubscriptionEvent::Complete));
        assert_eq!(state.status(), Some("ACCEPTED"));
    }

    #[test]
    fn every_order_status_has_a_hero_and_unknown_has_none() {
        for status in [
            "PLACED", "ACCEPTED", "REJECTED", "PREPARING", "READY", "OUT_FOR_DELIVERY",
            "DELIVERED", "CANCELLED_BY_CUSTOMER", "CANCELLED_BY_RESTAURANT",
        ] {
            assert!(status_hero(status).is_some(), "no hero for {status}");
        }
        assert_eq!(status_hero("NOT_A_STATUS"), None);
        // Spot-check the spec table: DELIVERED = home icon; both cancellations share the entry.
        assert_eq!(status_hero("DELIVERED").unwrap().icon, "home");
        assert_eq!(
            status_hero("CANCELLED_BY_CUSTOMER").unwrap(),
            status_hero("CANCELLED_BY_RESTAURANT").unwrap()
        );
    }

    #[tokio::test]
    async fn post_delivery_actions_dispatch_through_the_two_step_layer() {
        let acceptance = |mutation: &str| {
            Ok(json!({ mutation: {
                "messageId": "00000000-0000-7000-8000-000000000000",
                "correlationId": "00000000-0000-7000-8000-000000000000",
                "causeId": null, "sessionId": null, "traceId": null,
                "operationStatus": "PENDING", "duplicate": false,
            }}))
        };
        let fake = FakeTransport::scripted(vec![
            acceptance("rateOrder"),
            acceptance("recordDeliverySatisfaction"),
            acceptance("tipOrder"),
        ]);
        let id = Uuid::now_v7();

        rate_order(&fake, id, "rest-1", "UP").await.unwrap();
        record_delivery_satisfaction(&fake, id, "rest-1", "ON_TIME", None).await.unwrap();
        let tips = vec![TipLine { recipient: "RIDER".into(), amount_cents: 200, currency: "EUR".into() }];
        tip_order(&fake, id, "rest-1", &tips).await.unwrap();

        assert_eq!(fake.call(0).1["input"]["riderThumb"], "UP");
        let sat = fake.call(1).1;
        assert_eq!(sat["input"]["timeliness"], "ON_TIME");
        assert!(sat["input"].get("reason").is_none(), "reason only travels when given");
        let tip = fake.call(2).1;
        assert_eq!(tip["input"]["tips"][0]["amount"]["amountCents"], 200);
        assert_eq!(tip["input"]["tips"][0]["recipient"], "RIDER");
        // All three are two-step: each sent a minted messageId in metadata.
        for i in 0..3 {
            assert!(fake.call(i).1["metadata"]["messageId"].is_string());
        }
    }

    /// #420: the confirmation page is built from the resolved `order.byId`, not from `None`.
    #[test]
    fn from_resolved_takes_the_order_the_page_read_and_nothing_else() {
        let id = Uuid::now_v7();
        let mut data = Map::new();
        data.insert("order".into(), order("ACCEPTED", "2026-08-09T19:30:00Z"));
        assert_eq!(TrackingState::from_resolved(id, &data).status(), Some("ACCEPTED"));

        // A null read (stranger / unknown id / a transport that may not see this order) is the
        // honest not-found state — never an existence oracle, never a guessed status.
        data.insert("order".into(), Value::Null);
        assert_eq!(TrackingState::from_resolved(id, &data).order, None);
        assert_eq!(TrackingState::from_resolved(id, &Map::new()).order, None);
        assert_eq!(TrackingState::from_resolved(id, &Map::new()).order_id, id);
    }

    /// #420: the hero renders WORDS. It used to emit `data-i18n` on empty elements and nothing
    /// anywhere turned those into text, so the page a customer landed on after paying was blank
    /// above the fold. The body is withheld rather than shipped with an unfilled `{param}`.
    #[cfg(feature = "ssr")]
    #[test]
    fn the_status_hero_renders_a_sentence_in_both_languages() {
        for (locale, title, not_found) in [
            ("fr", "Commande acceptée", "Commande introuvable"),
            ("en", "Order accepted", "Order not found"),
        ] {
            let mut state = TrackingState::new(Uuid::now_v7());
            state.apply(&SubscriptionEvent::Next(order("ACCEPTED", "2026-08-09T19:30:00Z")));
            let html = render_tracking_html(state, locale);
            assert!(html.contains(title), "{locale}: {html}");
            assert!(!html.contains("[order.status."), "no `[key]` marker: {html}");
            // `order.status.accepted.body` is "{restaurant} prépare votre commande." and the
            // order.byId selection carries no restaurant name — so the body is omitted, not shipped
            // with a visible placeholder.
            assert!(!html.contains("{restaurant}"), "no unfilled param reaches a customer: {html}");

            let html = render_tracking_html(TrackingState::new(Uuid::now_v7()), locale);
            assert!(html.contains(not_found), "{locale}: {html}");
            assert!(!html.contains("[order.not_found]"), "{html}");
        }

        // A status whose copy needs nothing renders its body in full.
        let mut state = TrackingState::new(Uuid::now_v7());
        state.apply(&SubscriptionEvent::Next(order("DELIVERED", "2026-08-09T20:10:00Z")));
        let html = render_tracking_html(state, "fr");
        assert!(html.contains("Livré !"), "{html}");
        assert!(html.contains("Bon appétit."), "a param-free body renders: {html}");
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn tracking_renders_the_hero_state_machine() {
        let mut state = TrackingState::new(Uuid::now_v7());
        state.apply(&SubscriptionEvent::Next(order("PREPARING", "2026-07-23T12:05:00Z")));
        let html = render_tracking_html(state.clone(), "fr");
        assert!(html.contains("data-c=\"order_status_hero\""));
        assert!(html.contains("data-status=\"PREPARING\""));
        assert!(html.contains("data-icon=\"fire\""));
        assert!(html.contains("data-c=\"eta_bar\""), "PREPARING shows the ETA: {html}");
        assert!(!html.contains("rating_sheet"), "rating only offered when DELIVERED");

        state.apply(&SubscriptionEvent::Next(order("DELIVERED", "2026-07-23T13:00:00Z")));
        let html = render_tracking_html(state, "fr");
        assert!(html.contains("data-icon=\"home\""));
        assert!(!html.contains("data-c=\"eta_bar\""), "no ETA once DELIVERED");
        assert!(html.contains("rating_sheet"), "DELIVERED offers the rating sheet");

        // The empty state: no order loaded.
        let html = render_tracking_html(TrackingState::new(Uuid::now_v7()), "fr");
        assert!(html.contains("data-status=\"UNKNOWN\""));
    }
}
