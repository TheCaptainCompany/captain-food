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
//!     `data-on-success` navigation on success;
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

use crate::actions::{ActionOutcome, DispatchHandle};
use crate::executor::attrs;
use crate::generated::data_layer::{ActionKey, ActionKind};
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
/// success navigation. Registered under its push-subscription id.
struct InFlight {
    handle: DispatchHandle,
    button: web_sys::HtmlElement,
    original_label: String,
    on_success: Option<String>,
    settled: Rc<std::cell::Cell<bool>>,
}

struct Driver {
    transport: Rc<HttpTransport>,
    store: Rc<BrowserPendingStore>,
    /// The current WS handle (replaced on every reconnect by `on_connect`).
    socket: Rc<RefCell<Option<Handle>>>,
    /// Push-subscription id → the write it watches.
    in_flight: Rc<RefCell<HashMap<SubId, InFlight>>>,
}

/// Install the interaction layer: the delegated click listener, the shared subscription socket,
/// and the boot-time pending resume. Called once from `hydrate()`.
pub fn install(origin: &str, role: Role, session: SessionId) {
    let driver = Rc::new(Driver {
        transport: Rc::new(HttpTransport::new(origin, role, session)),
        store: Rc::new(BrowserPendingStore),
        socket: Rc::new(RefCell::new(None)),
        in_flight: Rc::new(RefCell::new(HashMap::new())),
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

    // The ONE delegated listener.
    {
        let d = Rc::clone(&driver);
        let listener = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |e: web_sys::MouseEvent| {
            let Some(target) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            else {
                return;
            };
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
        // Form-field bindings (#94): a var whose `{{ <field>.value }}` binding had no screen data
        // is filled from the LIVE input by its element id at dispatch time.
        if let Some(bindings) = el
            .get_attribute(attrs::VAR_BINDINGS)
            .and_then(|raw| serde_json::from_str::<Map<String, Value>>(&raw).ok())
        {
            for (name, binding) in bindings {
                let Some(field_id) = binding.as_str().and_then(|b| b.strip_suffix(".value")) else {
                    continue;
                };
                if let Some(value) = input_value(field_id) {
                    vars.insert(name, Value::String(value));
                }
            }
        }
        let on_success = el.get_attribute(attrs::ON_SUCCESS);
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
                    apply_outcome(&el, &original_label, &on_success, &outcome);
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
    fn on_push(&self, sub_id: SubId, event: SubscriptionEvent) {
        let SubscriptionEvent::Next(operation) = event else { return };
        let mut in_flight = self.in_flight.borrow_mut();
        let Some(watch) = in_flight.get(&sub_id) else { return };
        match pending::settle_from_push(self.store.as_ref(), &watch.handle, &operation) {
            Ok(Some(outcome)) => {
                watch.settled.set(true);
                apply_outcome(&watch.button, &watch.original_label, &watch.on_success, &outcome);
                let watch = in_flight.remove(&sub_id);
                drop(in_flight);
                if let (Some(_), Some(socket)) = (watch, self.socket.borrow_mut().as_mut()) {
                    socket.unsubscribe(sub_id);
                }
            }
            Ok(None) => {}  // PENDING frame — keep watching
            Err(_) => {}    // malformed push — the poll fallback owns the verdict
        }
    }
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

fn apply_outcome(
    el: &web_sys::HtmlElement,
    original_label: &str,
    on_success: &Option<String>,
    outcome: &ActionOutcome,
) {
    restore(el, original_label);
    match outcome {
        ActionOutcome::Succeeded { .. } => {
            let _ = el.set_attribute("data-state", "succeeded");
            if let (Some(route), Some(w)) = (on_success, web_sys::window()) {
                let _ = w.location().set_href(route);
            }
        }
        other => toast(&outcome_toast(other, "")),
    }
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
