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
}

pub fn auth_routes(state: AuthRoutesState) -> Router {
    Router::new()
        .route("/auth/session", post(exchange_session))
        .route("/auth/refresh", post(refresh_session))
        .route("/auth/logout", post(logout))
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
