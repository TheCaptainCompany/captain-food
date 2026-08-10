//! Request-transport envelope extraction (ADR-20260720-015500): the anonymous session identity and
//! the W3C trace context travel as HTTP headers — `X-SESSION-ID` (client-generated UUID, kept in a
//! cookie / app cache; identifies anonymous users end-to-end) and `traceparent` (the 32-hex
//! trace-id). Both are injected into GraphQL execution data next to the verified `Principal`; the
//! generated dispatch reads them into the command-journal envelope, and the ownership scopes of
//! `operationStatus`/`paymentStatus` compare against them.
//!
//! On the WebSocket leg browsers cannot set headers, so `X-SESSION-ID` is ALSO read from the
//! `connection_init` payload (same convention as `Authorization`, ADR-0047), with the upgrade
//! request's headers as fallback.

use axum::http::HeaderMap;

/// The `X-SESSION-ID` header, when present.
pub const SESSION_HEADER: &str = "x-session-id";
/// The W3C trace-context header.
pub const TRACEPARENT_HEADER: &str = "traceparent";

/// The validated anonymous-session identity of this request (`None` when the header is absent).
#[derive(Debug, Clone, Copy)]
pub struct SessionHeader(pub Option<uuid::Uuid>);

/// The W3C trace-id of this request (`None` when no/invalid `traceparent` came in).
#[derive(Debug, Clone)]
pub struct TraceContext(pub Option<String>);

/// The ONE correlation id of this request — `request.correlation_id`, the source every READ-path
/// observability contract names (`cart-price`, `read-authorization`).
///
/// Reads carry no command envelope, so nothing upstream supplies a correlation id; the server
/// mints it, ONCE, at the transport boundary and injects it into the GraphQL execution data. Every
/// read-path span in that request records the SAME value, which is the entire point: a customer
/// whose cart rendered without a price and who then abandoned checkout is one `business.correlation_id`
/// away from being findable. A span that mints its own id per call correlates to nothing — it is
/// the attribute present and the capability absent.
///
/// Minted per REQUEST, not per operation and not per resolver call: a `carts` read prices N carts
/// and all N `cart.price` spans must share the id.
#[derive(Debug, Clone, Copy)]
pub struct RequestCorrelationId(pub uuid::Uuid);

impl RequestCorrelationId {
    /// Mint this request's id. Called exactly once per request, at the transport boundary.
    pub fn mint() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

/// Why a present `X-SESSION-ID` was rejected (fail-visible: a malformed session id is a client bug
/// and must 400, never be silently dropped into anonymous-without-session).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidSessionHeader;

/// Extract + validate `X-SESSION-ID`: absent → `SessionHeader(None)`; present but not a UUID →
/// `Err(InvalidSessionHeader)` (the HTTP layer answers 400).
pub fn session_header(headers: &HeaderMap) -> Result<SessionHeader, InvalidSessionHeader> {
    match headers.get(SESSION_HEADER).map(|v| v.to_str().unwrap_or_default().trim()) {
        None => Ok(SessionHeader(None)),
        Some("") => Err(InvalidSessionHeader),
        Some(raw) => uuid::Uuid::parse_str(raw).map(|u| SessionHeader(Some(u))).map_err(|_| InvalidSessionHeader),
    }
}

/// Extract the trace-id from `traceparent` (`00-<32 hex trace-id>-<16 hex span-id>-<flags>`); a
/// missing or malformed header is simply no trace context (tracing is best-effort, never a 4xx).
pub fn trace_context(headers: &HeaderMap) -> TraceContext {
    let trace_id = headers
        .get(TRACEPARENT_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|tp| tp.split('-').nth(1))
        .filter(|id| id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit()) && *id != "00000000000000000000000000000000")
        .map(|id| id.to_ascii_lowercase());
    TraceContext(trace_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn absent_session_header_is_no_session() {
        assert!(matches!(session_header(&HeaderMap::new()), Ok(SessionHeader(None))));
    }

    #[test]
    fn valid_session_header_parses() {
        let mut h = HeaderMap::new();
        let id = uuid::Uuid::new_v4();
        h.insert(SESSION_HEADER, HeaderValue::from_str(&id.to_string()).unwrap());
        assert_eq!(session_header(&h).unwrap().0, Some(id));
    }

    #[test]
    fn malformed_session_header_is_rejected() {
        let mut h = HeaderMap::new();
        h.insert(SESSION_HEADER, HeaderValue::from_static("not-a-uuid"));
        assert!(session_header(&h).is_err());
        h.insert(SESSION_HEADER, HeaderValue::from_static(""));
        assert!(session_header(&h).is_err());
    }

    #[test]
    fn traceparent_extracts_the_trace_id() {
        let mut h = HeaderMap::new();
        h.insert(
            TRACEPARENT_HEADER,
            HeaderValue::from_static("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        );
        assert_eq!(trace_context(&h).0.as_deref(), Some("4bf92f3577b34da6a3ce929d0e0e4736"));
        // Malformed / all-zero trace ids are no trace context, never an error.
        h.insert(TRACEPARENT_HEADER, HeaderValue::from_static("garbage"));
        assert!(trace_context(&h).0.is_none());
        h.insert(
            TRACEPARENT_HEADER,
            HeaderValue::from_static("00-00000000000000000000000000000000-00f067aa0ba902b7-01"),
        );
        assert!(trace_context(&h).0.is_none());
    }
}
