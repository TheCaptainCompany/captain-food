//! GraphQL subscriptions — the PUSH side of the SDUI data layer (split 3/4 of #21).
//!
//! Speaks **graphql-transport-ws** (the subprotocol async-graphql serves on `GET /{role}/graphql`,
//! `crates/server/src/graphql/routes.rs`). Structured sans-IO, mirroring the read/write layers'
//! seam-first design:
//!
//!   * [`WsClient`] — the PROTOCOL STATE MACHINE, pure text-in/reactions-out: it owns the
//!     `connection_init` → `connection_ack` handshake, subscription ids, `next`/`error`/`complete`
//!     routing and `ping`/`pong`, but never touches a socket. Every protocol rule is therefore
//!     unit-testable natively with zero network — the same reason `Transport` exists.
//!   * The browser driver (`hydrate`, wasm32) — a thin loop owning one `web_sys::WebSocket`,
//!     feeding frames through the state machine and shipping its reactions back out.
//!
//! Auth rides the `connection_init` payload, NOT headers: a browser cannot set headers on a
//! WebSocket, so the server reads `Authorization` and `X-SESSION-ID` from the init payload (the
//! graphql-ws convention — see the server's `on_connection_init`). The session id is REQUIRED for
//! the same reason it is on every HTTP call: anonymous ownership scoping (`paymentStatusChanged`
//! for a guest checkout) keys on it.
//!
//! Free-tier reality (stated on the server route): the socket dies on instance restarts. The
//! contract for consumers is therefore **subscribe + re-sync**: on every (re)connect the client
//! re-issues its subscriptions AND the owning screen re-reads its pull query (`order.byId`,
//! `paymentStatus.byOrder`) — the push stream is an accelerator, never the only source of truth.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::generated::data_layer::{subscription_input_type, ResolverKey};
use crate::session::SessionId;

/// The allowlisted subscriptions — hand-written (the screens DSL binds subscriptions per screen via
/// `subscription: $ref`, not through the `resolvers` map, so no generated enum exists yet). Each
/// reuses the GENERATED selection set of the resolver returning the same api.yaml type, so the
/// pushed shape and the pulled shape can never drift apart — the fold in `tracking.rs` relies on
/// replace semantics between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubscriptionKey {
    /// `orderStatusChanged(orderId)` → Order (#14: keyed by what the confirmation route holds).
    OrderStatusChanged,
    /// `paymentStatusChanged(orderId)` → PaymentIntent (checkout: clientSecret + terminal status).
    PaymentStatusChanged,
    /// `operationStatusChanged(messageId)` → Operation (the push counterpart of the poll loop).
    OperationStatusChanged,
}

impl SubscriptionKey {
    /// The api.yaml subscription operation name.
    pub fn operation(&self) -> &'static str {
        match self {
            SubscriptionKey::OrderStatusChanged => "orderStatusChanged",
            SubscriptionKey::PaymentStatusChanged => "paymentStatusChanged",
            SubscriptionKey::OperationStatusChanged => "operationStatusChanged",
        }
    }

    /// The selection set — REUSED from the generated resolver returning the same type (Order /
    /// PaymentIntent / Operation), which is what keeps push and pull structurally identical.
    pub fn selection(&self) -> &'static str {
        let key = match self {
            SubscriptionKey::OrderStatusChanged => ResolverKey::OrderById,
            SubscriptionKey::PaymentStatusChanged => ResolverKey::PaymentStatusByOrder,
            SubscriptionKey::OperationStatusChanged => ResolverKey::OperationStatusByMessage,
        };
        key.selection().expect("subscription-backing resolvers always select an object type")
    }

    /// The subscription document: a single `input` variable whose type name is READ from the
    /// generated data layer (#97, `subscription_input_type` — the schema emitter's own naming),
    /// never re-derived by convention.
    pub fn document(&self) -> String {
        let operation = self.operation();
        let input_type = subscription_input_type(operation)
            .expect("every allowlisted subscription takes a required-args input");
        format!(
            "subscription Subscribe($input: {input_type}!) {{ {operation}(input: $input) {} }}",
            self.selection()
        )
    }
}

/// Client-assigned id of one live subscription on the socket (unique per connection, per protocol).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubId(u64);

impl SubId {
    fn wire(&self) -> String {
        self.0.to_string()
    }
}

/// One protocol-level happening on a live subscription, routed to its [`SubId`].
#[derive(Debug, Clone, PartialEq)]
pub enum SubscriptionEvent {
    /// A `next` frame: the operation's data subtree (already unwrapped from `data.<operation>`).
    Next(Value),
    /// An `error` frame: the subscription is DEAD server-side (contract/authz failure — business
    /// rejections never travel here, acceptance-first). Carries the raw errors for the log line.
    Failed(String),
    /// A `complete` frame: the server closed this subscription (end of stream, not an error).
    Complete,
}

/// What the state machine wants done after ingesting one frame. The driver executes these in order;
/// none of them can fail meaningfully client-side (a send on a closed socket surfaces as the
/// socket's own close event, which the driver already handles as a reconnect).
#[derive(Debug, Clone, PartialEq)]
pub enum Reaction {
    /// Send this text frame (a `pong` reply, or the flushed `subscribe` frames on ack).
    Send(String),
    /// The handshake completed — queued subscriptions were flushed; the connection is usable.
    Ready,
    /// Something happened on a live subscription.
    Event { id: SubId, event: SubscriptionEvent },
}

/// A protocol violation by the server (or a middlebox): the driver's only sane move is to drop the
/// socket and reconnect through the backoff policy.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("unparseable graphql-transport-ws frame: {0}")]
    Malformed(String),
    #[error("server refused the connection (connection_error / close before ack): {0}")]
    Refused(String),
}

/// Connection lifecycle: init sent → ack received. Subscriptions requested before the ack are
/// QUEUED and flushed on ack — the protocol forbids `subscribe` before `connection_ack`.
#[derive(Debug, PartialEq)]
enum Phase {
    AwaitingAck,
    Ready,
}

/// The graphql-transport-ws client state machine (sans-IO). One instance per socket ATTEMPT —
/// reconnecting means a fresh `WsClient` (the protocol has no session resume; resubscription is the
/// caller's re-sync duty, stated in the module docs).
pub struct WsClient {
    phase: Phase,
    next_id: u64,
    /// Subscriptions requested before ack, flushed in request order on `connection_ack`.
    queued: Vec<(SubId, String)>,
    /// Live subscriptions by wire id (u64 keys, but the wire speaks strings).
    active: BTreeMap<u64, SubscriptionKey>,
}

impl WsClient {
    /// Start a connection: the client + the `connection_init` frame the driver must send FIRST.
    /// `auth` is the `Bearer …` value for authenticated roles (`None` on the PUBLIC path); the
    /// session id always travels — anonymous ownership scoping depends on it.
    pub fn connect(auth: Option<&str>, session: SessionId) -> (Self, String) {
        let mut payload = Map::new();
        if let Some(token) = auth {
            payload.insert("Authorization".into(), json!(token));
        }
        payload.insert("X-SESSION-ID".into(), json!(session.to_string()));
        let init = json!({ "type": "connection_init", "payload": payload }).to_string();
        (
            Self { phase: Phase::AwaitingAck, next_id: 1, queued: Vec::new(), active: BTreeMap::new() },
            init,
        )
    }

    /// Request a subscription. Returns the [`SubId`] plus the `subscribe` frame to send NOW when
    /// the handshake is done — `None` means it was queued and will come back as a [`Reaction::Send`]
    /// on ack (the protocol forbids subscribing pre-ack).
    pub fn subscribe(
        &mut self,
        key: SubscriptionKey,
        variables: Map<String, Value>,
    ) -> (SubId, Option<String>) {
        let id = SubId(self.next_id);
        self.next_id += 1;
        let frame = json!({
            "type": "subscribe",
            "id": id.wire(),
            "payload": { "query": key.document(), "variables": { "input": variables } },
        })
        .to_string();
        self.active.insert(id.0, key);
        match self.phase {
            Phase::Ready => (id, Some(frame)),
            Phase::AwaitingAck => {
                self.queued.push((id, frame));
                (id, None)
            }
        }
    }

    /// Stop one subscription: the `complete` frame to send. The id is forgotten immediately —
    /// late frames for it are ignored (the protocol allows in-flight crossings).
    pub fn unsubscribe(&mut self, id: SubId) -> String {
        self.active.remove(&id.0);
        json!({ "type": "complete", "id": id.wire() }).to_string()
    }

    /// Ingest one incoming text frame; returns the reactions in execution order.
    pub fn handle(&mut self, text: &str) -> Result<Vec<Reaction>, ProtocolError> {
        let frame: Value =
            serde_json::from_str(text).map_err(|e| ProtocolError::Malformed(e.to_string()))?;
        let ty = frame
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| ProtocolError::Malformed("frame without a type".into()))?;

        match ty {
            "connection_ack" => {
                self.phase = Phase::Ready;
                let mut reactions = vec![Reaction::Ready];
                reactions.extend(self.queued.drain(..).map(|(_, frame)| Reaction::Send(frame)));
                Ok(reactions)
            }
            // Keepalive: answer every ping; ignore stray pongs (we never ping in V0 — the browser's
            // own TCP keepalive + the reconnect loop cover liveness).
            "ping" => Ok(vec![Reaction::Send(json!({ "type": "pong" }).to_string())]),
            "pong" => Ok(vec![]),
            "next" => Ok(self.routed(&frame, |sub, key| {
                // Unwrap `payload.data.<operation>` — same envelope rule as the HTTP transport:
                // anything in `errors` fails the whole frame (acceptance-first leaves nothing
                // business-meaningful there).
                let payload = frame.get("payload");
                if let Some(errors) = payload
                    .and_then(|p| p.get("errors"))
                    .filter(|e| e.as_array().is_some_and(|a| !a.is_empty()))
                {
                    return Reaction::Event {
                        id: sub,
                        event: SubscriptionEvent::Failed(errors.to_string()),
                    };
                }
                let data = payload
                    .and_then(|p| p.get("data"))
                    .and_then(|d| d.get(key.operation()))
                    .cloned()
                    .unwrap_or(Value::Null);
                Reaction::Event { id: sub, event: SubscriptionEvent::Next(data) }
            })),
            "error" => Ok(self.routed(&frame, |sub, _| {
                let errors =
                    frame.get("payload").map(|p| p.to_string()).unwrap_or_else(|| "[]".into());
                Reaction::Event { id: sub, event: SubscriptionEvent::Failed(errors) }
            })),
            "complete" => {
                let reactions = self.routed(&frame, |sub, _| Reaction::Event {
                    id: sub,
                    event: SubscriptionEvent::Complete,
                });
                // Terminal either way — forget the id.
                if let Some(Reaction::Event { id, .. }) = reactions.first() {
                    self.active.remove(&id.0);
                }
                Ok(reactions)
            }
            other => Err(ProtocolError::Malformed(format!("unexpected frame type `{other}`"))),
        }
    }

    /// Route a frame carrying an `id` to its live subscription; frames for unknown/forgotten ids
    /// dissolve silently (allowed in-flight crossings after `unsubscribe`).
    fn routed(
        &self,
        frame: &Value,
        f: impl FnOnce(SubId, SubscriptionKey) -> Reaction,
    ) -> Vec<Reaction> {
        frame
            .get("id")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok())
            .and_then(|raw| self.active.get(&raw).map(|key| f(SubId(raw), *key)))
            .into_iter()
            .collect()
    }
}

/// Reconnect pacing for the browser driver: exponential backoff, bounded — the free-tier server
/// restarts (module docs) make disconnects NORMAL, so the client must come back, but a hard cap
/// keeps a dead server from being hammered.
pub mod backoff {
    use std::time::Duration;

    /// First retry delay after a drop.
    pub const INITIAL: Duration = Duration::from_secs(1);
    /// Ceiling — with doubling: 1s, 2s, 4s, 8s, 16s, 30s, 30s, ...
    pub const MAX: Duration = Duration::from_secs(30);

    /// Delay before reconnect attempt `n` (1-based).
    pub fn delay(attempt: u32) -> Duration {
        let exp = INITIAL.saturating_mul(2u32.saturating_pow(attempt.saturating_sub(1)));
        exp.min(MAX)
    }
}

/// The browser driver (`hydrate`, wasm32): one `web_sys::WebSocket` in the `graphql-transport-ws`
/// subprotocol, frames pumped through a [`WsClient`], [`Reaction::Event`]s delivered to the
/// screen's callback. Reconnects through [`backoff`] on close; per the module contract the
/// `on_connect` callback runs on every (re)connect so the screen can resubscribe AND re-sync its
/// pull queries.
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub mod browser {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    use super::*;

    /// The subprotocol async-graphql negotiates for this dialect.
    pub const SUBPROTOCOL: &str = "graphql-transport-ws";

    /// The subscriptions endpoint for a role: `wss://{host}/{role}/graphql` (same URL as HTTP —
    /// the server upgrades on the WS handshake).
    pub fn endpoint(origin: &str, role: crate::graphql::Role) -> String {
        let ws_origin = origin
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1);
        format!("{}/{}/graphql", ws_origin.trim_end_matches('/'), role.segment())
    }

    /// A live, self-reconnecting subscription connection. Dropping the handle leaks the socket
    /// callbacks deliberately (`Closure::forget`) — V0 screens hold their connection for their
    /// whole lifetime; connection teardown lands with the router (split 4).
    pub struct Connection;

    impl Connection {
        /// Open the socket and drive it. `on_connect` is invoked with the fresh [`WsClient`]-backed
        /// [`Handle`] on EVERY successful handshake (first connect and every reconnect) —
        /// subscribe + re-sync there. `on_event` receives every subscription event.
        pub fn open(
            url: String,
            auth: Option<String>,
            session: SessionId,
            on_connect: Rc<dyn Fn(&mut Handle)>,
            on_event: Rc<dyn Fn(SubId, SubscriptionEvent)>,
        ) -> Connection {
            spawn_socket(url, auth, session, on_connect, on_event, 0);
            Connection
        }
    }

    /// The subscribing surface handed to `on_connect`: wraps the state machine + the socket so a
    /// screen can request subscriptions without seeing either.
    pub struct Handle {
        client: Rc<RefCell<WsClient>>,
        socket: web_sys::WebSocket,
    }

    impl Handle {
        /// Subscribe; frames queued pre-ack are flushed by the ack handler.
        pub fn subscribe(
            &mut self,
            key: SubscriptionKey,
            variables: serde_json::Map<String, Value>,
        ) -> SubId {
            let (id, frame) = self.client.borrow_mut().subscribe(key, variables);
            if let Some(frame) = frame {
                let _ = self.socket.send_with_str(&frame);
            }
            id
        }
    }

    fn spawn_socket(
        url: String,
        auth: Option<String>,
        session: SessionId,
        on_connect: Rc<dyn Fn(&mut Handle)>,
        on_event: Rc<dyn Fn(SubId, SubscriptionEvent)>,
        attempt: u32,
    ) {
        let Ok(socket) = web_sys::WebSocket::new_with_str(&url, SUBPROTOCOL) else {
            schedule_reconnect(url, auth, session, on_connect, on_event, attempt + 1);
            return;
        };

        let (client, init) = WsClient::connect(auth.as_deref(), session);
        let client = Rc::new(RefCell::new(client));

        // onopen: the protocol demands connection_init as the first frame.
        {
            let socket_open = socket.clone();
            let onopen = Closure::<dyn FnMut()>::new(move || {
                let _ = socket_open.send_with_str(&init);
            });
            socket.set_onopen(Some(onopen.as_ref().unchecked_ref()));
            onopen.forget();
        }

        // onmessage: pump every frame through the state machine, execute its reactions.
        {
            let socket_out = socket.clone();
            let client = Rc::clone(&client);
            let on_connect = Rc::clone(&on_connect);
            let on_event = Rc::clone(&on_event);
            let onmessage = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
                move |e: web_sys::MessageEvent| {
                    let Some(text) = e.data().as_string() else { return };
                    let reactions = match client.borrow_mut().handle(&text) {
                        Ok(r) => r,
                        // Protocol violation: drop the socket; onclose drives the reconnect.
                        Err(_) => {
                            let _ = socket_out.close();
                            return;
                        }
                    };
                    for reaction in reactions {
                        match reaction {
                            Reaction::Send(frame) => {
                                let _ = socket_out.send_with_str(&frame);
                            }
                            Reaction::Ready => {
                                let mut handle = Handle {
                                    client: Rc::clone(&client),
                                    socket: socket_out.clone(),
                                };
                                on_connect(&mut handle);
                            }
                            Reaction::Event { id, event } => on_event(id, event),
                        }
                    }
                },
            );
            socket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            onmessage.forget();
        }

        // onclose: reconnect through the backoff policy — disconnects are NORMAL (free tier).
        {
            let onclose = Closure::<dyn FnMut(web_sys::CloseEvent)>::new(
                move |_e: web_sys::CloseEvent| {
                    schedule_reconnect(
                        url.clone(),
                        auth.clone(),
                        session,
                        Rc::clone(&on_connect),
                        Rc::clone(&on_event),
                        attempt + 1,
                    );
                },
            );
            socket.set_onclose(Some(onclose.as_ref().unchecked_ref()));
            onclose.forget();
        }
    }

    fn schedule_reconnect(
        url: String,
        auth: Option<String>,
        session: SessionId,
        on_connect: Rc<dyn Fn(&mut Handle)>,
        on_event: Rc<dyn Fn(SubId, SubscriptionEvent)>,
        attempt: u32,
    ) {
        let delay = backoff::delay(attempt);
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(delay.as_millis().min(u32::MAX as u128) as u32)
                .await;
            spawn_socket(url, auth, session, on_connect, on_event, attempt);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn session() -> SessionId {
        SessionId::from_request(Uuid::now_v7())
    }

    fn ack(client: &mut WsClient) -> Vec<Reaction> {
        client.handle(r#"{"type":"connection_ack"}"#).unwrap()
    }

    #[test]
    fn connection_init_carries_session_and_optional_auth() {
        let s = session();
        let (_, init) = WsClient::connect(Some("Bearer tok"), s);
        let frame: Value = serde_json::from_str(&init).unwrap();
        assert_eq!(frame["type"], "connection_init");
        assert_eq!(frame["payload"]["Authorization"], "Bearer tok");
        assert_eq!(frame["payload"]["X-SESSION-ID"], s.to_string());

        // PUBLIC path: no Authorization key at all (not a null — the server treats presence as a token).
        let (_, init) = WsClient::connect(None, s);
        let frame: Value = serde_json::from_str(&init).unwrap();
        assert!(frame["payload"].get("Authorization").is_none());
    }

    #[test]
    fn subscribe_before_ack_is_queued_and_flushed_on_ack() {
        let (mut client, _) = WsClient::connect(None, session());
        let mut vars = Map::new();
        vars.insert("orderId".into(), json!("order-1"));
        let (id, frame) = client.subscribe(SubscriptionKey::OrderStatusChanged, vars);
        assert!(frame.is_none(), "the protocol forbids subscribe before connection_ack");

        let reactions = ack(&mut client);
        assert_eq!(reactions[0], Reaction::Ready);
        let Reaction::Send(flushed) = &reactions[1] else { panic!("queued subscribe not flushed") };
        let sent: Value = serde_json::from_str(flushed).unwrap();
        assert_eq!(sent["type"], "subscribe");
        assert_eq!(sent["id"], id.wire());
        // The document follows the api.yaml conventions and reuses the generated Order selection.
        let query = sent["payload"]["query"].as_str().unwrap();
        assert!(query.contains("$input: OrderStatusChangedSubscriptionInput!"), "{query}");
        assert!(query.contains("orderStatusChanged(input: $input)"), "{query}");
        assert!(query.contains("deliveryTimeliness"), "must reuse the OrderById selection: {query}");
        assert_eq!(sent["payload"]["variables"]["input"]["orderId"], "order-1");
    }

    #[test]
    fn subscribe_after_ack_sends_immediately() {
        let (mut client, _) = WsClient::connect(None, session());
        ack(&mut client);
        let (_, frame) = client.subscribe(SubscriptionKey::PaymentStatusChanged, Map::new());
        assert!(frame.is_some(), "post-ack subscribe must be immediate");
    }

    #[test]
    fn next_frames_route_to_their_subscription_and_unwrap_the_operation() {
        let (mut client, _) = WsClient::connect(None, session());
        ack(&mut client);
        let (id, _) = client.subscribe(SubscriptionKey::OrderStatusChanged, Map::new());

        let reactions = client
            .handle(&json!({
                "type": "next",
                "id": id.wire(),
                "payload": { "data": { "orderStatusChanged": { "id": "o1", "status": "ACCEPTED" } } },
            }).to_string())
            .unwrap();
        assert_eq!(
            reactions,
            vec![Reaction::Event {
                id,
                event: SubscriptionEvent::Next(json!({ "id": "o1", "status": "ACCEPTED" })),
            }]
        );
    }

    #[test]
    fn error_and_complete_are_distinct_terminal_events() {
        let (mut client, _) = WsClient::connect(None, session());
        ack(&mut client);
        let (id, _) = client.subscribe(SubscriptionKey::OperationStatusChanged, Map::new());

        let failed = client
            .handle(&json!({ "type": "error", "id": id.wire(), "payload": [{ "message": "forbidden" }] }).to_string())
            .unwrap();
        assert!(matches!(
            &failed[0],
            Reaction::Event { event: SubscriptionEvent::Failed(msg), .. } if msg.contains("forbidden")
        ));

        // complete for the same id: the id was NOT forgotten by `error` (protocol keeps that the
        // server's move), so complete still routes — and then forgets it.
        let complete = client
            .handle(&json!({ "type": "complete", "id": id.wire() }).to_string())
            .unwrap();
        assert_eq!(complete, vec![Reaction::Event { id, event: SubscriptionEvent::Complete }]);
        let late = client
            .handle(&json!({ "type": "next", "id": id.wire(), "payload": { "data": {} } }).to_string())
            .unwrap();
        assert!(late.is_empty(), "frames after complete must dissolve silently");
    }

    #[test]
    fn frames_after_unsubscribe_dissolve_silently() {
        let (mut client, _) = WsClient::connect(None, session());
        ack(&mut client);
        let (id, _) = client.subscribe(SubscriptionKey::OrderStatusChanged, Map::new());
        let frame = client.unsubscribe(id);
        let sent: Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(sent["type"], "complete");

        let late = client
            .handle(&json!({ "type": "next", "id": id.wire(), "payload": { "data": { "orderStatusChanged": {} } } }).to_string())
            .unwrap();
        assert!(late.is_empty());
    }

    #[test]
    fn ping_is_answered_with_pong() {
        let (mut client, _) = WsClient::connect(None, session());
        let reactions = client.handle(r#"{"type":"ping"}"#).unwrap();
        assert_eq!(reactions, vec![Reaction::Send(r#"{"type":"pong"}"#.into())]);
    }

    #[test]
    fn next_frame_carrying_graphql_errors_fails_that_subscription_event() {
        let (mut client, _) = WsClient::connect(None, session());
        ack(&mut client);
        let (id, _) = client.subscribe(SubscriptionKey::PaymentStatusChanged, Map::new());
        let reactions = client
            .handle(&json!({
                "type": "next",
                "id": id.wire(),
                "payload": { "data": null, "errors": [{ "message": "resolver blew up" }] },
            }).to_string())
            .unwrap();
        assert!(matches!(
            &reactions[0],
            Reaction::Event { event: SubscriptionEvent::Failed(msg), .. } if msg.contains("resolver blew up")
        ));
    }

    #[test]
    fn malformed_and_unknown_frames_are_protocol_errors() {
        let (mut client, _) = WsClient::connect(None, session());
        assert!(matches!(client.handle("not json"), Err(ProtocolError::Malformed(_))));
        assert!(matches!(
            client.handle(r#"{"type":"start"}"#), // legacy graphql-ws dialect — not ours
            Err(ProtocolError::Malformed(_))
        ));
    }

    #[test]
    fn backoff_doubles_and_caps() {
        use std::time::Duration;
        assert_eq!(backoff::delay(1), Duration::from_secs(1));
        assert_eq!(backoff::delay(2), Duration::from_secs(2));
        assert_eq!(backoff::delay(4), Duration::from_secs(8));
        assert_eq!(backoff::delay(10), Duration::from_secs(30), "capped");
    }
}
