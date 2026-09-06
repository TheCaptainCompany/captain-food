//! The interaction driver (#93, `hydrate`-only) — the thin wasm glue over the pure layers.
//!
//! ONE delegated `click` listener on the document drives every SDUI button: the renderer stamped
//! each button's parsed action onto data attributes (`executor::attrs` — the DOM contract), so the
//! driver reads the CLOSEST `[data-action]` element and:
//!
//!   * **client kinds** — navigate / `tel:` dial directly; sheet + clipboard + share effects are
//!     re-emitted as a `captain:action` DOM CustomEvent (the sheet host lands with #94 — an event
//!     nobody handles yet is still visible in devtools, never a silent swallow);
//!   * **mutation kinds** — the full two-step UX: disable + loading label →
//!     `pending::dispatch_persisted` → verdict push-first (`operationStatusChanged` on the shared
//!     socket, interpreted by `pending::settle_from_push`) with the bounded poll as fallback →
//!     restore + toast on REJECTED/FAILED (server-provided message, errors.yaml code as fallback) →
//!     on SUCCESS, the `data-on-success` ORDERED STEPS run in declared order (#529 —
//!     `executor::run_on_success`: claim the parked session cookie, open/close a sheet, navigate —
//!     `ClaimSession` is `.await`ed before whatever follows it);
//!   * **retry** — a transport failure BEFORE acceptance keeps the persisted record and stamps its
//!     messageId onto the button (`data-retry`): the next click goes through `pending::retry`
//!     (same id — duplicate-proof) instead of minting a new intent.
//!
//! Boot: [`install`] runs `pending::resume_pending` — settled intents from a previous page
//! lifetime surface as toasts; still-open ones stay stored.
//!
//! Everything decision-shaped lives in the native-tested modules (`executor`, `pending`,
//! `actions`); this file is DOM plumbing by design.

#![cfg(all(target_arch = "wasm32", feature = "hydrate"))]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Map, Value};
use uuid::Uuid;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::actions::{ActionError, ActionOutcome, DispatchHandle};
use crate::executor::{attrs, OnSuccessStep};
use crate::generated::data_layer::{ActionKey, ActionKind};
use crate::generated::screens::Screen;
use crate::graphql::{HttpTransport, Role};
use crate::pending::{self, BrowserPendingStore, PendingStore, ResumedWrite};
use crate::session::SessionId;
use crate::subscriptions::browser::{endpoint, Connection, Handle};
use crate::subscriptions::{SubId, SubscriptionEvent, SubscriptionKey};

/// How long the fallback poll waits before its first read once a write is in flight — long enough
/// for the push path to win in the common case, short enough that a dead socket costs one head
/// start, not a stall (the poll's own bounded loop takes over from there).
const PUSH_HEAD_START: Duration = Duration::from_secs(2);

/// A dispatched write the driver is tracking: the acceptance handle + the button to restore + the
/// ordered `on_success` steps (#529 — `executor::OnSuccessStep`, no longer a single route).
/// Registered under its push-subscription id.
struct InFlight {
    handle: DispatchHandle,
    button: web_sys::HtmlElement,
    original_label: String,
    on_success: Vec<OnSuccessStep>,
    settled: Rc<std::cell::Cell<bool>>,
}

struct Driver {
    transport: Rc<crate::graphql::RefreshingTransport>,
    store: Rc<BrowserPendingStore>,
    /// The current WS handle (replaced on every reconnect by `on_connect`).
    socket: Rc<RefCell<Option<Handle>>>,
    /// Push-subscription id → the write it watches.
    in_flight: Rc<RefCell<HashMap<SubId, InFlight>>>,
    /// The BFF origin (#529 — `OnSuccessStep::ClaimSession` POSTs `{origin}/auth/session`).
    origin: String,
    /// This tab's anonymous session id (#529 — the `X-SESSION-ID` `claim_session` must present to
    /// match the one that journaled `verify_otp`).
    session: SessionId,
    /// The screen this page mounted (#639 part C step 4-ii, ADR-20260904-124600 §2): a refused
    /// Tell's bounce decision (`crate::bounce::bounce_target`) needs the SAME screen's declared
    /// routes the hydrate loop reads — one screen per page, so one `&'static Screen` for the
    /// driver's whole lifetime.
    screen: &'static Screen,
    /// This page's `pathname` + `search` at install time (#904 D2): the same value the hydrate
    /// loop's own bounce composes `?next=` from — read ONCE here since this page never navigates
    /// without a fresh `hydrate()` run.
    current_path_and_query: String,
}

/// Install the interaction layer: the delegated click listener, the shared subscription socket,
/// and the boot-time pending resume. Called once from `hydrate()`. `screen` is the matched screen
/// of THIS page — the bounce decision on a refused Tell reads its `restricted_route`/
/// `unauthenticated_route`, the same pair the hydrate loop's refused READS already read.
/// `refresh_used` (#904, ADR-20260905-101349 §13) is the SAME one-shot-refresh flag `hydrate()`'s
/// read transport shares, so a refresh failure is remembered for the WHOLE page — reads and
/// mutations alike — not re-attempted the moment a button click follows a failed read.
pub fn install(
    origin: &str,
    role: Role,
    session: SessionId,
    screen: &'static Screen,
    refresh_used: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let current_path_and_query = web_sys::window()
        .map(|w| {
            let l = w.location();
            format!("{}{}", l.pathname().unwrap_or_default(), l.search().unwrap_or_default())
        })
        .unwrap_or_default();
    let driver = Rc::new(Driver {
        transport: Rc::new(crate::graphql::RefreshingTransport::new(
            Box::new(HttpTransport::new(origin, role, session)),
            Box::new(crate::graphql::HttpRefresher::new(origin, session)),
            refresh_used,
        )),
        store: Rc::new(BrowserPendingStore),
        socket: Rc::new(RefCell::new(None)),
        in_flight: Rc::new(RefCell::new(HashMap::new())),
        origin: origin.to_string(),
        session,
        screen,
        current_path_and_query,
    });

    // The shared push socket. `on_connect` fires on every (re)connect: store the fresh handle —
    // in-flight writes on the OLD socket fall back to their poll (the re-sync contract).
    {
        let socket_slot = Rc::clone(&driver.socket);
        let d = Rc::clone(&driver);
        Connection::open(
            endpoint(origin, role),
            None, // auth token wiring lands with #94; PUBLIC-path pushes work today
            session,
            Rc::new(move |handle: &mut Handle| {
                *socket_slot.borrow_mut() = Some(handle.clone());
            }),
            Rc::new(move |sub_id, event| d.on_push(sub_id, event)),
        );
    }

    // The delegated CLICK listener — buttons, and chip selection (#114).
    {
        let d = Rc::clone(&driver);
        let listener = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |e: web_sys::MouseEvent| {
            let Some(target) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            else {
                return;
            };
            // A chip pick (#114): stash the value in the group's hidden input BEFORE dispatching,
            // so the fieldset action's `{{ <group>.value }}` binding fills from it. Fires the #62
            // survey (the timeliness chips). Then fall through to the group's data-action.
            if let Some(chip) = target.closest("[data-chip-value]").ok().flatten() {
                if let (Some(value), Some(group)) =
                    (chip.get_attribute("data-chip-value"), chip.get_attribute("data-chip-group"))
                {
                    if let Some(input) = web_sys::window()
                        .and_then(|w| w.document())
                        .and_then(|doc| doc.get_element_by_id(&group))
                        .and_then(|el| el.dyn_into::<web_sys::HtmlInputElement>().ok())
                    {
                        input.set_value(&value);
                    }
                }
            }
            let Some(el) = target
                .closest("[data-action]")
                .ok()
                .flatten()
                .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
            else {
                return;
            };
            d.on_click(el);
        });
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            let _ = doc.add_event_listener_with_callback("click", listener.as_ref().unchecked_ref());
        }
        listener.forget();
    }

    // The delegated INPUT listener — `on_complete` auto-submit (#114): an `otp_input` that reaches
    // its declared length dispatches (e.g. the 6th OTP digit fires verify_otp).
    {
        let d = Rc::clone(&driver);
        let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let Some(el) = e
                .target()
                .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                .filter(|el| el.get_attribute(attrs::TRIGGER).as_deref() == Some("complete"))
            else {
                return;
            };
            let Some(len) = el.get_attribute(attrs::COMPLETE_LEN).and_then(|s| s.parse::<usize>().ok())
            else {
                return;
            };
            let filled = el
                .dyn_ref::<web_sys::HtmlInputElement>()
                .map(|i| i.value().chars().count())
                .unwrap_or(0);
            if filled >= len && el.get_attribute("data-busy").is_none() {
                d.on_click(el);
            }
        });
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            let _ = doc.add_event_listener_with_callback("input", listener.as_ref().unchecked_ref());
        }
        listener.forget();
    }

    // Boot resume: settled intents from a previous page lifetime become toasts; open ones stay.
    {
        let d = Rc::clone(&driver);
        wasm_bindgen_futures::spawn_local(async move {
            let resumed = pending::resume_pending(
                d.transport.as_ref(),
                d.store.as_ref(),
                2,
                crate::actions::POLL_INTERVAL,
            )
            .await;
            for r in resumed {
                match r {
                    ResumedWrite::Settled { write, outcome } => {
                        toast(&outcome_toast(&outcome, write.action.as_str()));
                    }
                    ResumedWrite::StillOpen { .. } => {} // stays stored for the next boot / retry
                }
            }
        });
    }
}

impl Driver {
    fn on_click(self: &Rc<Self>, el: web_sys::HtmlElement) {
        let attr = |name: &str| el.get_attribute(name);
        let Some(action) = attr(attrs::ACTION) else { return };
        let Some(key) = ActionKey::from_key(&action) else { return };

        match key.kind() {
            ActionKind::Mutation => self.dispatch_mutation(el, key),
            ActionKind::Client => match action.as_str() {
                "navigate" => {
                    if let (Some(route), Some(w)) = (attr(attrs::ROUTE), web_sys::window()) {
                        let _ = w.location().set_href(&route);
                    }
                }
                "phone_call" => {
                    if let (Some(number), Some(w)) = (attr(attrs::NUMBER), web_sys::window()) {
                        let _ = w.location().set_href(&format!("tel:{number}"));
                    }
                }
                // The sheet host (#94): sheets render HIDDEN with `data-sheet-id`; open/close
                // toggle the `hidden` attribute.
                "open_bottom_sheet" => {
                    if let Some(sheet_id) = attr(attrs::SHEET) {
                        set_sheet_hidden(Some(&sheet_id), false);
                    }
                }
                "close_sheet" => set_sheet_hidden(None, true),
                // Clipboard/share: re-emitted as CustomEvents — visible, not swallowed.
                _ => emit_action_event(&el, &action),
            },
            // auth/gap render disabled — a click can only mean the DOM was tampered with; ignore.
            ActionKind::Auth | ActionKind::Gap => {}
        }
    }

    fn dispatch_mutation(self: &Rc<Self>, el: web_sys::HtmlElement, key: ActionKey) {
        if el.get_attribute("data-busy").is_some() {
            return; // double-tap guard: the idempotency story handles retries, not double UI flows
        }
        let mut vars: Map<String, Value> = el
            .get_attribute(attrs::VARS)
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        // Unresolved variables (`data-var-bindings`) are filled here, at dispatch time — two seams
        // over the same list:
        if let Some(bindings) = el
            .get_attribute(attrs::VAR_BINDINGS)
            .and_then(|raw| serde_json::from_str::<Map<String, Value>>(&raw).ok())
        {
            // #147: dispatch-time synthesized tokens, over the reported unresolved bindings. Both
            // are persisted with the pending write below; a same-op retry re-sends the stored input
            // (keeping the values), a new user action synthesizes fresh ones.
            let synth_tokens: Vec<(String, String)> = bindings
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect();
            // `{{ $uuid }}` -> a fresh UUIDv7 idempotency key.
            crate::executor::fill_mint_tokens(&mut vars, &synth_tokens, Uuid::now_v7);
            // `{{ $locale }}` -> the client's current UI locale (the #110 `<html lang>`).
            crate::executor::fill_locale_tokens(&mut vars, &synth_tokens, &current_locale());
            // Form-field bindings (#94): a var whose `{{ <field>.value }}` binding had no screen
            // data is filled from the LIVE input by its element id. (`$uuid` tokens don't end in
            // `.value`, so this pass skips them — the two seams never overlap.)
            for (name, binding) in bindings {
                let Some(field_id) = binding.as_str().and_then(|b| b.strip_suffix(".value")) else {
                    continue;
                };
                if let Some(value) = input_value(field_id) {
                    vars.insert(name, Value::String(value));
                }
            }
        }
        let on_success = crate::executor::parse_on_success_attr(
            &el.get_attribute(attrs::ON_SUCCESS).unwrap_or_default(),
        );
        let retry_id = el.get_attribute("data-retry").and_then(|s| Uuid::parse_str(&s).ok());

        // Pending UX: freeze the button.
        let original_label = el.inner_text();
        if let Some(loading) = el.get_attribute(attrs::LOADING) {
            el.set_inner_text(&loading);
        }
        let _ = el.set_attribute("data-busy", "true");
        el.set_class_name(&format!("{} is-pending", el.class_name()));

        let d = Rc::clone(self);
        wasm_bindgen_futures::spawn_local(async move {
            // Same-id retry when a previous click failed before acceptance; fresh intent otherwise.
            let dispatched = match retry_id
                .and_then(|id| d.store.load().into_iter().find(|w| w.message_id == id))
            {
                Some(write) => pending::retry(d.transport.as_ref(), &write).await,
                None => {
                    pending::dispatch_persisted(d.transport.as_ref(), d.store.as_ref(), key, vars)
                        .await
                }
            };

            let handle = match dispatched {
                Ok(h) => {
                    let _ = el.remove_attribute("data-retry");
                    h
                }
                Err(err) => {
                    // #639 part C step 4-ii (ADR-20260904-124600 §2), extended by #904 D2/D4: a
                    // refused Tell bounces the SAME way a refused read does — never a toast for a
                    // rider mid-job who just got restricted — and this 401 has ALREADY been through
                    // the one-shot refresh-and-reissue inside `d.transport` (a `RefreshingTransport`,
                    // #904 D1): reaching this branch at all means the refresh either failed or had
                    // already been spent this page, so the ORIGINAL messageId's mutation is never
                    // re-dispatched a second time from here — no new id, no toast, no retap.
                    // `ActionError::Transport` is the ONLY variant that can carry the signal (a
                    // client/auth/gap/unbound refusal never reaches the transport).
                    if let ActionError::Transport(t) = &err {
                        if let Some(route) =
                            crate::bounce::bounce_target(t, d.screen, &d.current_path_and_query)
                        {
                            navigate_to(&route);
                            return;
                        }
                    }
                    // Pre-acceptance failure: the record (if any) is stamped for a same-id retry.
                    if let Some(w) = d.store.load().into_iter().find(|w| w.action == key) {
                        let _ = el.set_attribute("data-retry", &w.message_id.to_string());
                    }
                    restore(&el, &original_label);
                    toast(&format!("Network problem — tap to retry ({err})"));
                    return;
                }
            };

            let settled = Rc::new(std::cell::Cell::new(false));

            // Push-first: watch operationStatusChanged for this messageId on the shared socket.
            if let Some(socket) = d.socket.borrow_mut().as_mut() {
                let mut vars = Map::new();
                vars.insert("messageId".into(), Value::String(handle.message_id.to_string()));
                let sub_id = socket.subscribe(SubscriptionKey::OperationStatusChanged, vars);
                d.in_flight.borrow_mut().insert(
                    sub_id,
                    InFlight {
                        handle,
                        button: el.clone(),
                        original_label: original_label.clone(),
                        on_success: on_success.clone(),
                        settled: Rc::clone(&settled),
                    },
                );
            }

            // Fallback poll: give the push a head start, then the bounded loop is the guarantee.
            crate::actions::sleep(PUSH_HEAD_START).await;
            if settled.get() {
                return;
            }
            match pending::settle(d.transport.as_ref(), d.store.as_ref(), &handle).await {
                Ok(outcome) if !settled.get() => {
                    settled.set(true);
                    apply_outcome(&d, &el, &original_label, &on_success, handle.message_id, &outcome)
                        .await;
                }
                Ok(_) => {} // push won while we polled — already applied
                Err(err) if !settled.get() => {
                    restore(&el, &original_label);
                    toast(&format!("Still processing — retry is safe ({err})"));
                }
                Err(_) => {}
            }
        });
    }

    /// A frame from the shared socket: route it to its in-flight write, settle push-first.
    /// `self: &Rc<Self>` (like `on_click`) — `apply_outcome`'s `ClaimSession` step is genuinely
    /// async, so the outcome is applied on a SPAWNED task, after everything needed is cloned out of
    /// the `in_flight` borrow (never held across an `.await`).
    fn on_push(self: &Rc<Self>, sub_id: SubId, event: SubscriptionEvent) {
        let SubscriptionEvent::Next(operation) = event else { return };
        let mut in_flight = self.in_flight.borrow_mut();
        let Some(watch) = in_flight.get(&sub_id) else { return };
        match pending::settle_from_push(self.store.as_ref(), &watch.handle, &operation) {
            Ok(Some(outcome)) => {
                watch.settled.set(true);
                let button = watch.button.clone();
                let original_label = watch.original_label.clone();
                let on_success = watch.on_success.clone();
                let message_id = watch.handle.message_id;
                let removed = in_flight.remove(&sub_id);
                drop(in_flight);
                if let (Some(_), Some(socket)) = (removed, self.socket.borrow_mut().as_mut()) {
                    socket.unsubscribe(sub_id);
                }
                let d = Rc::clone(self);
                wasm_bindgen_futures::spawn_local(async move {
                    apply_outcome(&d, &button, &original_label, &on_success, message_id, &outcome).await;
                });
            }
            Ok(None) => {}  // PENDING frame — keep watching
            Err(_) => {}    // malformed push — the poll fallback owns the verdict
        }
    }
}

/// The client's current UI locale, read from the shell's `<html lang>` (#110 stamps the resolved
/// locale there; the hydrate re-render reads it back the same way). Empty/absent falls back to the
/// supported default locale — the `{{ $locale }}` token always resolves to a valid supported tag.
fn current_locale() -> String {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
        .and_then(|e| e.get_attribute("lang"))
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| crate::i18n::DEFAULT_LOCALE.to_string())
}

/// The live value of a form field by element id (`{{ <id>.value }}` bindings).
fn input_value(field_id: &str) -> Option<String> {
    let doc = web_sys::window()?.document()?;
    let el = doc.get_element_by_id(field_id)?;
    el.dyn_into::<web_sys::HtmlInputElement>().ok().map(|i| i.value())
}

/// Toggle sheet visibility: `Some(id)` shows THAT sheet (hiding the others — one sheet at a
/// time), `None` + hide=true closes them all.
fn set_sheet_hidden(open_id: Option<&str>, hide_all: bool) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    let Ok(sheets) = doc.query_selector_all("[data-sheet-id]") else { return };
    for i in 0..sheets.length() {
        let Some(el) = sheets.get(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else {
            continue;
        };
        let is_target = open_id.is_some_and(|id| el.get_attribute("data-sheet-id").as_deref() == Some(id));
        if is_target && !hide_all {
            let _ = el.remove_attribute("hidden");
        } else {
            let _ = el.set_attribute("hidden", "");
        }
    }
}

fn restore(el: &web_sys::HtmlElement, label: &str) {
    el.set_inner_text(label);
    let _ = el.remove_attribute("data-busy");
    el.set_class_name(&el.class_name().replace(" is-pending", ""));
}

/// Apply a settled mutation's outcome: restore the button, then on SUCCESS run the declared
/// `on_success` steps IN ORDER (#529 — `executor::run_on_success`; `ClaimSession` is the only
/// genuinely async one, `.await`ed before whatever follows it runs), otherwise toast the failure.
async fn apply_outcome(
    driver: &Driver,
    el: &web_sys::HtmlElement,
    original_label: &str,
    on_success: &[OnSuccessStep],
    message_id: uuid::Uuid,
    outcome: &ActionOutcome,
) {
    restore(el, original_label);
    match outcome {
        ActionOutcome::Succeeded { .. } => {
            let _ = el.set_attribute("data-state", "succeeded");
            let origin = driver.origin.clone();
            let session = driver.session;
            crate::executor::run_on_success(
                on_success,
                || {
                    let origin = origin.clone();
                    async move {
                        let _ = crate::auth::claim_session(&origin, message_id, session).await;
                    }
                },
                |route| navigate_home_or_next(route),
                |sheet_id| set_sheet_hidden(Some(sheet_id), false),
                || set_sheet_hidden(None, true),
            )
            .await;
        }
        other => {
            // #639 2c-ii: a screen that declared WHERE this action's refusal lands
            // (`inline_error` with `for_action`) shows it there — the reason stays on screen
            // beside the one route out, in the caller's language (the server localized it);
            // otherwise the toast, as before.
            let text = outcome_toast(other, "");
            let action = el.get_attribute(attrs::ACTION).unwrap_or_default();
            if !reveal_inline_error(&action, &text) {
                toast(&text);
            }
        }
    }
}

/// Fill and un-hide the `inline_error` declared for `action` (`data-for-action`), if the screen
/// has one. Returns whether a slot took the message.
fn reveal_inline_error(action: &str, text: &str) -> bool {
    if action.is_empty() {
        return false;
    }
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return false };
    let selector = format!("[data-c=\"inline_error\"][data-for-action=\"{action}\"]");
    let Some(slot) = doc.query_selector(&selector).ok().flatten() else { return false };
    slot.set_text_content(Some(text));
    let _ = slot.remove_attribute("hidden");
    true
}

/// `navigate` step target (#529): the special [`crate::executor::RELOAD_ROUTE`] token reloads the
/// current location (a fresh SSR pass re-reads the now-set auth cookie); anything else is a literal
/// href.
fn navigate_to(route: &str) {
    let Some(w) = web_sys::window() else { return };
    if route == crate::executor::RELOAD_ROUTE {
        let _ = w.location().reload();
    } else {
        let _ = w.location().set_href(route);
    }
}

/// #904 D3 — the "rider door" leg (no email hop, same tab throughout): the `on_success` chain's
/// generic home navigation (declared `route == "/"`) honors a pending `?next=` still present in
/// the CURRENT location's query string — read directly, NEVER `sessionStorage` (that store is the
/// EMAIL-HOP legs' mechanism, `sign_in_return.rs`/`admin_sign_in_return.rs`; here the page never
/// navigated away, so the value is still right there in the URL). Any OTHER declared route is left
/// exactly as the screen author wrote it — this never second-guesses an explicit destination, only
/// the generic "go home" one. A renderer rule, no spec field (ADR-20260817-105845): the SDUI
/// `navigate` grammar is untouched, this is plain Rust deciding which literal href to hand it.
fn navigate_home_or_next(route: &str) {
    if route == "/" {
        if let Some(target) = pending_next_in_current_location() {
            navigate_to(&target);
            return;
        }
    }
    navigate_to(route);
}

/// The validated `next` a stranger's URL is STILL carrying right now, if any — never consumed
/// (nothing to remove: this reads the live location, not a store), never twice-decoded (`safe_next`
/// decodes once).
fn pending_next_in_current_location() -> Option<String> {
    let window = web_sys::window()?;
    let location = window.location();
    let host = location.host().ok()?;
    let search = location.search().ok()?;
    let raw = crate::next_param::extract_next(&search)?;
    crate::router::safe_next(&host, &raw).map(str::to_string)
}

/// The user-facing line for a non-success outcome: the server's message when present (it is the
/// localized business text), the stable errors.yaml code as the fallback — never silence.
fn outcome_toast(outcome: &ActionOutcome, action: &str) -> String {
    match outcome {
        ActionOutcome::Succeeded { .. } => {
            if action.is_empty() { "Done".to_string() } else { format!("{action}: done") }
        }
        ActionOutcome::Rejected { error_code, message, .. } => {
            message.clone().unwrap_or_else(|| error_code.clone())
        }
        ActionOutcome::Failed { message, .. } => message
            .clone()
            .unwrap_or_else(|| "Something went wrong — retry is safe".to_string()),
    }
}

/// Sheet/clipboard/share handoff: a bubbling `captain:action` CustomEvent carrying the action key
/// (detail) — the sheet host (#94) listens; until then it is inspectable, never swallowed.
fn emit_action_event(el: &web_sys::Element, action: &str) {
    let init = web_sys::CustomEventInit::new();
    init.set_bubbles(true);
    init.set_detail(&wasm_bindgen::JsValue::from_str(action));
    if let Ok(event) = web_sys::CustomEvent::new_with_event_init_dict("captain:action", &init) {
        let _ = el.dispatch_event(&event);
    }
}

/// The toast: one shared element under `<body>`, `data-c="toast_notification"` (the registered
/// chrome kind), auto-hidden after a beat.
fn toast(text: &str) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    let el = match doc.get_element_by_id("captain-toast") {
        Some(el) => el,
        None => {
            let Ok(el) = doc.create_element("div") else { return };
            el.set_id("captain-toast");
            let _ = el.set_attribute("data-c", "toast_notification");
            if let Some(body) = doc.body() {
                let _ = body.append_child(&el);
            }
            el
        }
    };
    el.set_text_content(Some(text));
    let _ = el.set_attribute("data-visible", "true");
    let el = el.clone();
    wasm_bindgen_futures::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(5_000).await;
        let _ = el.remove_attribute("data-visible");
    });
}
