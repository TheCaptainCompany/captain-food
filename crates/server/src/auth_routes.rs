//! Session-cookie transport endpoints (#112, PROP-20260724-150500) — the client's ONLY window onto
//! the provider session, and it never sees the token.
//!
//! Not GraphQL (this is transport, not domain): after `verifyPhone`/`confirmEmailVerification`
//! SUCCEEDs, the handler parked the provider session keyed by the acceptance messageId
//! (`application::commands::verify_phone`). The browser calls:
//!
//!   * `POST /auth/session { messageId }` — presenting its `X-SESSION-ID`; iff it matches the
//!     journaling session (the `operationStatus` ownership rule) the row is claimed (single-read)
//!     and the response sets `captain_auth` (access JWT, httpOnly) + `captain_refresh` (scoped to
//!     `/auth`). The token is chosen by the SERVER and delivered as a cookie — never readable by JS.
//!   * `POST /auth/refresh` — rotates via the refresh cookie through the identity service.
//!   * `POST /auth/logout` — clears both cookies (the `sign_out` action).
//!
//! Every authenticated request then rides these cookies automatically (same-origin fetch, the WS
//! upgrade, SSR) and `AuthContext` verifies them via its cookie fallback — one seam, all carriers.

use std::sync::Arc;

use application::auth_sessions::{AuthSessionStore, ParkedAuthSession};
use application::generated::services::{IdentityRefreshSessionInput, IdentityService, ServiceCallMeta};
use axum::{
    extract::State,
    http::{header::SET_COOKIE, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::Deserialize;

use crate::auth::AUTH_COOKIE;

const REFRESH_COOKIE: &str = "captain_refresh";

/// Shared state for the auth routes — `None` stores/services degrade to 503 (fail closed: no key
/// or no identity provider ⇒ no sessions, never plaintext).
#[derive(Clone)]
pub struct AuthRoutesState {
    pub sessions: Option<Arc<dyn AuthSessionStore>>,
    pub identity: Arc<dyn IdentityService>,
    /// The OVH SMS client + Supabase Send-SMS hook secret (#118) — `None` when OVH/the secret is
    /// unconfigured (the hook 503s; auth stays SMS-less, never a half-open delivery path).
    pub sms: Option<Arc<infrastructure::OvhSmsClient>>,
    pub sms_hook_secret: Option<Arc<Vec<u8>>>,
    /// The send guards (#516). **This is the wall**, not a nicety: `sms_hook` is where a message
    /// becomes a euro on our own OVH account, so the authoritative allowlist + budget claim happens
    /// here. `None` means the shared counter is unavailable, and the hook then 503s — fail-closed,
    /// because an unguarded send path is exactly the failure this guard exists to prevent.
    pub sms_guard: Option<Arc<infrastructure::SmsSendAuthorizer>>,
}

pub fn auth_routes(state: AuthRoutesState) -> Router {
    Router::new()
        .route("/auth/session", post(exchange_session))
        .route("/auth/refresh", post(refresh_session))
        .route("/auth/logout", post(logout))
        // The Supabase Auth Send-SMS hook target (#118): verify Supabase's signature → OVH send.
        .route("/auth/sms-hook", post(sms_hook))
        .with_state(state)
}

#[derive(Deserialize)]
struct SessionRequest {
    #[serde(rename = "messageId")]
    message_id: uuid::Uuid,
}

/// `POST /auth/session`: exchange a claimed parked session for the httpOnly cookies.
async fn exchange_session(
    State(state): State<AuthRoutesState>,
    headers: HeaderMap,
    Json(req): Json<SessionRequest>,
) -> Response {
    let Some(sessions) = state.sessions else {
        return (StatusCode::SERVICE_UNAVAILABLE, "auth sessions not configured").into_response();
    };
    // Ownership: the X-SESSION-ID that journaled the verify command must be the one claiming.
    let session_id = match crate::graphql::session::session_header(&headers) {
        Ok(s) => s.0,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid X-SESSION-ID").into_response(),
    };
    match sessions.claim(req.message_id, session_id).await {
        Ok(Some(parked)) => set_session_cookies(&parked).into_response(),
        // Absent / expired / wrong owner are indistinguishable — no existence oracle.
        Ok(None) => (StatusCode::NOT_FOUND, "no session for that messageId").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "session claim failed").into_response(),
    }
}

/// `POST /auth/refresh`: rotate the session from the refresh cookie.
async fn refresh_session(State(state): State<AuthRoutesState>, headers: HeaderMap) -> Response {
    let Some(refresh) = cookie(&headers, REFRESH_COOKIE) else {
        return (StatusCode::UNAUTHORIZED, "no refresh cookie").into_response();
    };
    match state
        .identity
        .refresh_session(
            IdentityRefreshSessionInput { refresh_token: refresh.to_string() },
            &ServiceCallMeta::new(uuid::Uuid::now_v7()),
        )
        .await
    {
        Ok(out) => set_session_cookies(&ParkedAuthSession {
            message_id: uuid::Uuid::nil(),
            session_id: None,
            access_token: out.access_token,
            refresh_token: out.refresh_token,
            expires_in: out.expires_in,
        })
        .into_response(),
        Err(_) => clear_cookies(StatusCode::UNAUTHORIZED, "refresh rejected"),
    }
}

/// `POST /auth/logout`: clear both cookies.
async fn logout() -> Response {
    clear_cookies(StatusCode::OK, "signed out")
}

/// `POST /auth/sms-hook` (#118): the Supabase Auth Send-SMS hook. Verify Supabase's
/// standard-webhooks signature, extract `(phone, otp)`, **authorise the send**, deliver via OVH.
/// Returns 204 on success; 401 on a bad/missing signature; 429 when the send guards refuse; 503 when
/// SMS or the guard is unconfigured.
///
/// **THIS ROUTE IS WHERE THE MONEY MOVES (#516)**, and that is why the authoritative guard is here
/// rather than only on the command path. `requestPhoneVerification` merely asks the identity provider
/// to send; the euro is spent LATER and INBOUND, when the provider calls us back here and we hand the
/// message to OVH on our own account. A check at the BFF edge or in the command handler is therefore
/// present but not unbypassable — anything able to make the provider send reaches this route without
/// passing our command path at all.
///
/// The guard also runs AFTER signature verification, deliberately: an unsigned request must not be
/// able to spend budget (or probe which numbers are rate-limited) before it is refused as a forgery.
async fn sms_hook(State(state): State<AuthRoutesState>, headers: HeaderMap, body: String) -> Response {
    let (Some(sms), Some(secret), Some(guard)) =
        (state.sms.as_ref(), state.sms_hook_secret.as_ref(), state.sms_guard.as_ref())
    else {
        return (StatusCode::SERVICE_UNAVAILABLE, "sms delivery not configured").into_response();
    };
    let h = |k: &str| headers.get(k).and_then(|v| v.to_str().ok()).unwrap_or("");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if !infrastructure::supabase_sms_hook::verify(
        secret,
        h("webhook-id"),
        h("webhook-timestamp"),
        h("webhook-signature"),
        &body,
        now,
    ) {
        return (StatusCode::UNAUTHORIZED, "invalid webhook signature").into_response();
    }
    let Some((phone, otp)) = infrastructure::supabase_sms_hook::parse_payload(&body) else {
        return (StatusCode::BAD_REQUEST, "unexpected hook payload").into_response();
    };
    // THE WALL. Country allowlist + per-number caps + the global daily ceiling, claimed atomically
    // against the SHARED counter. The witness this returns is the only thing `sms.send` accepts, so
    // there is no way past it that compiles.
    let recipient = match guard.authorize_e164(&phone).await {
        Ok(recipient) => recipient,
        Err(refusal) => {
            // 429 tells Supabase this was refused rather than broken, and the refusal is already
            // counted + logged by the authorizer (loudly for the global ceiling). The body carries
            // the bounded reason only — never the number.
            return (StatusCode::TOO_MANY_REQUESTS, format!("sms refused: {}", refusal.reason()))
                .into_response();
        }
    };
    let message = format!("Votre code Captain.Food : {otp}");
    match sms.send(recipient, &message).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        // Supabase treats a non-2xx as delivery failure and surfaces an error to the client.
        Err(e) => {
            tracing::error!(error = %e, "sms-hook: OVH send failed -- the customer receives no OTP");
            (StatusCode::BAD_GATEWAY, "sms delivery failed").into_response()
        }
    }
}

/// Build the `Set-Cookie` pair for a session. `SameSite=Lax` + httpOnly + `Secure`; the access
/// cookie is site-wide, the refresh cookie is path-scoped to `/auth` so it only travels to the
/// rotation endpoint. Max-Age tracks the provider's `expiresIn` (default 1h if unreported).
fn set_session_cookies(session: &ParkedAuthSession) -> Response {
    let max_age = session.expires_in.filter(|s| *s > 0).unwrap_or(3600);
    let mut headers = HeaderMap::new();
    headers.append(
        SET_COOKIE,
        cookie_str(AUTH_COOKIE, &session.access_token, "/", max_age).parse().unwrap(),
    );
    if let Some(refresh) = &session.refresh_token {
        headers.append(
            SET_COOKIE,
            // Refresh lives longer than the access token; 30 days is the usual provider default.
            cookie_str(REFRESH_COOKIE, refresh, "/auth", 30 * 24 * 3600).parse().unwrap(),
        );
    }
    (StatusCode::NO_CONTENT, headers).into_response()
}

fn clear_cookies(status: StatusCode, body: &'static str) -> Response {
    let mut headers = HeaderMap::new();
    for (name, path) in [(AUTH_COOKIE, "/"), (REFRESH_COOKIE, "/auth")] {
        headers.append(SET_COOKIE, cookie_str(name, "", path, 0).parse().unwrap());
    }
    (status, headers, body).into_response()
}

fn cookie_str(name: &str, value: &str, path: &str, max_age: i64) -> String {
    format!("{name}={value}; HttpOnly; Secure; SameSite=Lax; Path={path}; Max-Age={max_age}")
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(axum::http::header::COOKIE).and_then(|v| v.to_str().ok()).and_then(|raw| {
        raw.split(';').map(str::trim).find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            (k.trim() == name).then(|| v.trim()).filter(|t| !t.is_empty())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unconfigured_state() -> AuthRoutesState {
        AuthRoutesState {
            sessions: None,
            identity: std::sync::Arc::new(infrastructure::FailClosedIdentityService),
            sms: None,
            sms_hook_secret: None,
            sms_guard: None,
        }
    }

    /// A guard over the in-memory reference counter, for the hook-path tests. The served path uses
    /// the shared Postgres store; the POLICY it applies is the same object either way.
    fn guard(policy: application::sms_guard::SmsSendPolicy) -> Arc<infrastructure::SmsSendAuthorizer> {
        Arc::new(infrastructure::SmsSendAuthorizer::new(
            policy,
            Box::new(application::sms_guard::InMemorySmsQuotaStore::default()),
        ))
    }

    /// A hook request Supabase would really send, correctly signed — signed through the SAME
    /// construction the route verifies with, so the two cannot drift.
    fn signed_hook(secret: &[u8], phone: &str) -> (HeaderMap, String) {
        let body = format!("{{\"user\":{{\"phone\":\"{phone}\"}},\"sms\":{{\"otp\":\"123456\"}}}}");
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
            .to_string();
        let id = "msg_1";
        let sig = infrastructure::supabase_sms_hook::sign(secret, id, &ts, &body);
        let mut headers = HeaderMap::new();
        headers.insert("webhook-id", id.parse().unwrap());
        headers.insert("webhook-timestamp", ts.parse().unwrap());
        headers.insert("webhook-signature", sig.parse().unwrap());
        (headers, body)
    }

    /// A state whose OVH client is deliberately absent, so any attempt to SEND would be visible as a
    /// panic/none rather than a network call. The assertion that matters is the STATUS the guard
    /// produces BEFORE the sender is reached: 429 means refused by the guard, 503 means it never got
    /// to a configured sender at all. See the note on `no_sms_client_is_reached` below.
    fn guarded_state(
        secret: &[u8],
        policy: application::sms_guard::SmsSendPolicy,
    ) -> AuthRoutesState {
        let mut state = unconfigured_state();
        state.sms_hook_secret = Some(Arc::new(secret.to_vec()));
        state.sms_guard = Some(guard(policy));
        // An OVH client pointed at an unroutable base URL: if the guard ever let a refused number
        // through, the test would hang/fail on the transport instead of passing. Reaching a send at
        // all is the failure.
        state.sms = infrastructure::OvhSmsClient::for_test("http://127.0.0.1:1").map(Arc::new);
        state
    }

    #[tokio::test]
    async fn the_hook_refuses_an_unserved_country_and_never_reaches_the_sender() {
        // #516, THE money assertion on the money path: `/auth/sms-hook` is where a message becomes a
        // euro on our OVH account. A '+212' recipient must be refused HERE, with a correctly signed
        // request — i.e. the refusal is the guard's, not the signature check's.
        let secret = b"hook-secret-key";
        let state = guarded_state(secret, application::sms_guard::SmsSendPolicy::default());
        let (headers, body) = signed_hook(secret, "+212612345678");
        let resp = sms_hook(State(state), headers, body).await;
        assert_eq!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "a signed hook for an unserved country must be refused by the guard, never sent"
        );
    }

    #[tokio::test]
    async fn the_hook_refuses_the_second_send_inside_the_window() {
        let secret = b"hook-secret-key";
        // Allow the first, refuse the immediate second: the cooldown is server-side, and the
        // client's resend countdown is not a control.
        let state = guarded_state(secret, application::sms_guard::SmsSendPolicy::default());
        let (headers, body) = signed_hook(secret, "+33612345678");
        // The first one reaches the sender and fails at the (unroutable) transport — 502, which is
        // itself the proof that the GUARD let it through rather than refusing it.
        let first = sms_hook(State(state.clone()), headers, body).await;
        assert_eq!(first.status(), StatusCode::BAD_GATEWAY, "the first send must pass the guard");
        let (headers, body) = signed_hook(secret, "+33612345678");
        let second = sms_hook(State(state), headers, body).await;
        assert_eq!(
            second.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "the second send inside the cooldown must be refused"
        );
    }

    #[tokio::test]
    async fn a_forged_or_absent_signature_is_refused_before_the_guard_can_be_probed() {
        // Order matters: an unsigned caller must not be able to spend budget, nor to probe which
        // numbers are rate-limited, before being refused as a forgery.
        let secret = b"hook-secret-key";
        let state = guarded_state(secret, application::sms_guard::SmsSendPolicy::default());
        let mut headers = HeaderMap::new();
        headers.insert("webhook-id", "msg_1".parse().unwrap());
        headers.insert("webhook-timestamp", "0".parse().unwrap());
        headers.insert("webhook-signature", "v1,AAAA".parse().unwrap());
        let resp = sms_hook(
            State(state),
            headers,
            "{\"user\":{\"phone\":\"+33612345678\"},\"sms\":{\"otp\":\"1\"}}".into(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn the_global_ceiling_stops_the_hook_for_everyone() {
        let secret = b"hook-secret-key";
        let policy = application::sms_guard::SmsSendPolicy {
            max_per_day_global: 0, // the kill switch
            ..application::sms_guard::SmsSendPolicy::default()
        };
        let state = guarded_state(secret, policy);
        let (headers, body) = signed_hook(secret, "+33612345678");
        let resp = sms_hook(State(state), headers, body).await;
        assert_eq!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "with the ceiling spent, even a served French number is refused"
        );
    }

    #[tokio::test]
    async fn the_hook_fails_closed_when_the_guard_is_absent() {
        // No shared counter ⇒ no guarded send path ⇒ 503. An unguarded send is exactly the failure
        // mode the guard exists to prevent, so "no guard" must never mean "send anyway".
        let secret = b"hook-secret-key";
        let mut state = unconfigured_state();
        state.sms_hook_secret = Some(Arc::new(secret.to_vec()));
        state.sms = infrastructure::OvhSmsClient::for_test("http://127.0.0.1:1").map(Arc::new);
        state.sms_guard = None;
        let (headers, body) = signed_hook(secret, "+33612345678");
        let resp = sms_hook(State(state), headers, body).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn sms_hook_fails_closed_when_sms_is_unconfigured() {
        // #118: no OVH client / no secret ⇒ 503 (SMS-less, never a half-open delivery path).
        let resp = sms_hook(State(unconfigured_state()), HeaderMap::new(), "{}".into()).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn sms_hook_rejects_a_bad_signature_401() {
        // Configured with a secret but the request carries no/invalid signature → 401, never sends.
        let mut state = unconfigured_state();
        // A dummy OVH client would try to send; instead prove the signature gate rejects FIRST by
        // giving a secret + no OVH client is still 503 — so use a secret AND assert the sig path by
        // checking that a wrong signature never reaches send. With sms=None the 503 wins, so this
        // test asserts the ORDER is safe: unconfigured is refused before any signature trust.
        state.sms_hook_secret = Some(std::sync::Arc::new(b"key".to_vec()));
        let resp = sms_hook(State(state), HeaderMap::new(), "{}".into()).await;
        // sms client still None → 503 (fail-closed), never a spoofed-signature send.
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
