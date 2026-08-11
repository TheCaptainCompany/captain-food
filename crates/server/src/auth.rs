//! API authentication + role authorization for the role-as-path GraphQL endpoints (ADR-0047, realizing the
//! deferred guard of ADR-0006 over ADR-0015 Supabase Auth).
//!
//! `/public/graphql` is open. Every other role path requires a valid Supabase **JWT**
//! (`Authorization: Bearer <token>`), verified against the project's **public** keys fetched from
//! `SUPABASE_JWKS_URL` — the signing secret never touches this server. The verified token yields a
//! [`Principal`]; the request's path role must equal the principal's `app_metadata.captain_role`
//! (absent ⇒ CUSTOMER), else `403`. Missing/invalid token on a non-public path ⇒ `401`; if JWKS is
//! unreachable we **fail closed** (`503`) rather than allow.
//!
//! **Open is not credential-blind (#469).** `/public` also READS whatever credential the request
//! carries, because the storefront IS the open path (`web::router` pins the customer surfaces to
//! `Role::Public`): without that, an identified customer arrives as `ReadScope::Public` and
//! `cart.current`'s claim leg is unreachable from a browser. It is the ONLY path that degrades
//! instead of refusing — invalid, expired, absent-verifier and non-CUSTOMER credentials all serve
//! the anonymous view with a `200` (see [`AuthContext::public_principal`]) — and it grants at most
//! the CUSTOMER identity: a staff token there is anonymous, never elevated.
//!
//! Security notes: the verification algorithm is taken from the matched **JWK** (not the attacker-controlled
//! header) and restricted to asymmetric families, closing the classic `alg`-confusion hole (an attacker
//! can't downgrade to HS256 and sign with the public key).

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use jsonwebtoken::{
    decode, decode_header,
    jwk::{Jwk, JwkSet, KeyAlgorithm},
    Algorithm, DecodingKey, Validation,
};
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::graphql::acl::RequestRole;

/// How long a fetched JWKS is trusted before a refresh (key rotation is also handled by a forced refetch
/// when a token's `kid` is not in the cached set).
const JWKS_TTL: Duration = Duration::from_secs(3600);
/// Hard ceiling on ONE JWKS fetch (see [`jwks_client`]): the open path now verifies credentials, so
/// an unbounded fetch would be an unbounded storefront request.
const JWKS_FETCH_TIMEOUT: Duration = Duration::from_secs(3);
/// Supabase issues user tokens with this audience.
const SUPABASE_AUDIENCE: &str = "authenticated";

/// The authenticated caller injected into the GraphQL context. `user_id` is the Supabase `sub` (`None` for
/// anonymous PUBLIC). `role` is the (verified) role this request is authorized to act as.
///
/// `allow(dead_code)`: the fields are consumed by resolvers and the per-field `@auth` guard (ADR-0006),
/// which are still deferred — the request already carries the verified identity so wiring them is a pure add.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Principal {
    pub user_id: Option<String>,
    pub role: RequestRole,
    /// Tenant claims (#144/#433), verified with the rest of the token — the login-to-domain bridge
    /// lives in JWT claims for EVERY role (ADR-20260809-050000 CARD-11; product-owner correction on
    /// #430: "this information is provided in the jwt"). The claim IS the domain id; the auth
    /// subject stays `sub` (PROP-20260725-185140 §3.2.1 — a binding, not a subject swap). No
    /// per-request lookup resolves read scope anywhere.
    ///
    /// KNOWN LIMITATION (proposal §6.4, register row "claim staleness", open by explicit decision):
    /// a claim is frozen until token refresh. For customers/riders the only transition is
    /// null -> set at mint time, so the #429 bearer-token item carries the BLOCKING precondition
    /// that the client obtains its token AFTER the claim stamp (or forces one refresh) — otherwise
    /// the first paid session is the one denied its tracking screen.
    pub restaurant_id: Option<uuid::Uuid>,
    pub restaurant_account_id: Option<uuid::Uuid>,
    pub customer_id: Option<uuid::Uuid>,
    pub rider_id: Option<uuid::Uuid>,
}

impl Principal {
    /// The unauthenticated PUBLIC identity — also what the SSR page renderer executes reads as
    /// (#92: a document GET carries no credentials, so server-side data resolution is by
    /// construction the anonymous view).
    pub(crate) fn anonymous() -> Self {
        Self {
            user_id: None,
            role: RequestRole::Public,
            restaurant_id: None,
            restaurant_account_id: None,
            customer_id: None,
            rider_id: None,
        }
    }

    /// The identified CUSTOMER on the OPEN path (#469): a verified `captain_auth` cookie / bearer
    /// presented to `/public/graphql`, which the storefront is pinned to (`web::router` serves the
    /// customer surfaces as `Role::Public`). Before #469 the open path skipped credential reading
    /// entirely, so `cart.current`'s claim leg could never fire from a browser.
    ///
    /// **Escalation is unspellable here, by construction, not by review**: this constructor takes
    /// the CUSTOMER claim and nothing else, and hard-sets `restaurant_id` /
    /// `restaurant_account_id` / `rider_id` to `None`. A token carrying `captain_restaurant_id`
    /// cannot leak a `ReadScope::Restaurant` onto the one path everyone can reach, whatever the
    /// caller sends — and a non-CUSTOMER `captain_role` never reaches this constructor at all (see
    /// [`AuthContext::public_principal`], which degrades it to [`Principal::anonymous`]).
    /// `role: Customer` is the PRINCIPAL's role, i.e. who the caller IS; the per-field ACL keeps
    /// running against the PATH role (`RequestRole::Public`), which is injected separately, so this
    /// widens no field's visibility — only the row scope of reads the open path already served.
    ///
    /// `customer_id` is **not optional** (reviewer S3): a CUSTOMER principal without its domain
    /// claim resolves to `ReadScope::Public` anyway — it serves no cart, matches no ownership and
    /// fires no leg — so on the open path that is not a weaker identity, it is a DEGRADE, and it is
    /// handled as one (`claim_absent`, see [`AuthContext::public_principal`]). Taking the id by
    /// value is what stops the claimless state being reconstructible here and quietly reaching
    /// `read_scope`'s provisioning-gap counter.
    pub(crate) fn public_customer(sub: String, customer_id: uuid::Uuid) -> Self {
        Self {
            user_id: Some(sub),
            role: RequestRole::Customer,
            restaurant_id: None,
            restaurant_account_id: None,
            customer_id: Some(customer_id),
            rider_id: None,
        }
    }
}

/// Why authorization failed, mapped to an HTTP status at the edge.
#[derive(Debug)]
pub enum AuthError {
    /// No/!malformed/invalid token on a non-public path.
    Unauthorized,
    /// Valid token, but its role is not permitted for this path.
    Forbidden,
    /// Auth cannot be performed (JWKS not configured or unreachable) — fail closed.
    Unavailable,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        match self {
            AuthError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "unauthorized: valid bearer token required").into_response()
            }
            AuthError::Forbidden => {
                (StatusCode::FORBIDDEN, "forbidden: token role not permitted for this path").into_response()
            }
            AuthError::Unavailable => {
                (StatusCode::SERVICE_UNAVAILABLE, "auth unavailable").into_response()
            }
        }
    }
}

/// Only the claims we consume. Reserved claims (`exp`/`aud`/`iss`) are validated by `jsonwebtoken` from the
/// raw payload via [`Validation`], so they need not appear here.
#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    #[serde(default)]
    app_metadata: AppMetadata,
}

#[derive(Debug, Default, Deserialize)]
struct AppMetadata {
    #[serde(default)]
    captain_role: Option<String>,
    /// The single location a RESTAURANT principal acts for (#144). Server-controlled `app_metadata`,
    /// exactly like `captain_role` — not user-editable, so it is trustworthy without a DB lookup.
    #[serde(default)]
    captain_restaurant_id: Option<String>,
    /// The account a RESTAURANT_ACCOUNT principal acts for (#144) — a chain's manager reaches every
    /// location under it. This is why the two roles exist as distinct UserTypes rather than one
    /// restaurant role with a multiplicity problem.
    #[serde(default)]
    captain_restaurant_account_id: Option<String>,
    /// The CUSTOMER's domain id (#433) — stamped when the Customer is registered/resolved
    /// (verifyPhone), never user-editable. Replaces the per-request `auth_ref` bridge for read
    /// scope; a token without it fails closed to Public.
    #[serde(default)]
    captain_customer_id: Option<String>,
    /// The RIDER's domain id (#433) — minted by the #415 per-person rider onboarding. Until that
    /// lands no rider token carries it, and rider reads fail closed.
    #[serde(default)]
    captain_rider_id: Option<String>,
}

struct CachedJwks {
    set: JwkSet,
    fetched: Instant,
}

/// Verifier state: the JWKS endpoint, the expected audience/issuer, an HTTP client, the cached key set,
/// and the pre-shared EXTERNAL service tokens (machine callers to `/external`).
pub struct AuthContext {
    jwks_url: Option<String>,
    issuer: Option<String>,
    /// Pre-shared secrets for EXTERNAL machine callers (Stripe/HubRise/Avelo37 ACLs), presented via the
    /// `X-External-Api-Key` header. Loaded from `EXTERNAL_API_TOKENS` (comma-separated). Empty ⇒ no
    /// service-token access to `/external` (a Supabase JWT with captain_role EXTERNAL still works).
    external_tokens: Vec<String>,
    http: reqwest::Client,
    cache: RwLock<Option<CachedJwks>>,
}

impl AuthContext {
    /// Build from the **resolved** configuration: `jwks_url` (public keys) and `supabase_url` (used to
    /// derive the expected `iss = {SUPABASE_URL}/auth/v1`) come from the generated `Config`, which applies
    /// precedence env > baked profile > default (ADR-20260729-020000). With no JWKS URL, only `/public`
    /// works; other paths return `503`.
    ///
    /// These two are **non-secret baked** config: on the deployed service they live inside the image, NOT
    /// in the process environment. Reading them straight from `std::env` here (as this did) made every
    /// authenticated path fail closed with `503 "auth unavailable"` in production, where the JWKS URL is
    /// baked into the digest, not set as a Render env var — the same trap `263f2a2` fixed for the smoke
    /// script. `EXTERNAL_API_TOKENS` stays an env read: it is a **secret**, delivered by CI into the
    /// service environment, and carries no baked value.
    pub fn from_config(jwks_url: String, supabase_url: String) -> Arc<Self> {
        let jwks_url = Some(jwks_url).filter(|s| !s.is_empty());
        let issuer = Some(supabase_url)
            .filter(|s| !s.is_empty())
            .map(|u| format!("{}/auth/v1", u.trim_end_matches('/')));
        if jwks_url.is_none() {
            tracing::warn!("SUPABASE_JWKS_URL resolved empty -- non-public GraphQL paths will return 503 (fail closed)");
        }
        let external_tokens: Vec<String> = std::env::var("EXTERNAL_API_TOKENS")
            .ok()
            .map(|s| s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect())
            .unwrap_or_default();
        Arc::new(Self {
            jwks_url,
            issuer,
            external_tokens,
            http: jwks_client(),
            cache: RwLock::new(None),
        })
    }

    /// Authorize a request for `path_role`. `/public` is always allowed and NEVER fails — it reads
    /// whatever credential is present and degrades to anonymous ([`Self::public_principal`]); every
    /// other path requires a valid bearer token whose `captain_role` equals `path_role`.
    pub async fn authorize(&self, path_role: RequestRole, headers: &HeaderMap) -> Result<Principal, AuthError> {
        if path_role == RequestRole::Public {
            return Ok(self.public_principal(headers).await);
        }
        // EXTERNAL machine callers (Stripe/HubRise/Avelo37 ACLs) present a pre-shared service token via
        // the `X-External-Api-Key` header instead of a user JWT. If a key header is present it is
        // authoritative (valid → allow, invalid → reject); if absent we fall through to JWT verification,
        // so a Supabase user carrying captain_role EXTERNAL still works.
        if path_role == RequestRole::External {
            if let Some(key) = headers.get("x-external-api-key").and_then(|v| v.to_str().ok()) {
                return if self.external_key_valid(key) {
                    Ok(Principal {
                        user_id: None,
                        role: RequestRole::External,
                        restaurant_id: None,
                        restaurant_account_id: None,
                        customer_id: None,
                        rider_id: None,
                    })
                } else {
                    Err(AuthError::Unauthorized)
                };
            }
        }
        let session_token = token(headers).ok_or(AuthError::Unauthorized)?;
        let claims = self.verify(session_token).await?;
        let granted = claims
            .app_metadata
            .captain_role
            .as_deref()
            .map(parse_role)
            .unwrap_or(RequestRole::Customer);
        if role_permitted(path_role, granted) {
            Ok(Principal {
                restaurant_id: claim_uuid(&claims.app_metadata.captain_restaurant_id),
                restaurant_account_id: claim_uuid(&claims.app_metadata.captain_restaurant_account_id),
                customer_id: claim_uuid(&claims.app_metadata.captain_customer_id),
                rider_id: claim_uuid(&claims.app_metadata.captain_rider_id),
                user_id: Some(claims.sub),
                role: path_role,
            })
        } else {
            Err(AuthError::Forbidden)
        }
    }

    /// The OPEN path's identity resolution (#469): attempt verification, **degrade to anonymous on
    /// every failure**. Returns a `Principal`, never a `Result` — `/public` cannot 401, 403 or 503,
    /// whatever arrives, because a stale cookie is the COMMON case and a JWKS outage must not take
    /// anonymous browsing down with it. Friday 19:00 with zero orders from an auth dependency the
    /// storefront never had is the failure this shape exists to prevent.
    ///
    /// Three degradations, all counted (`public_credential_degraded_total{reason}`) so the
    /// degradation is visible rather than silent:
    /// - **the credential does not verify** (absent-kid, expired, tampered, JWKS unreachable) —
    ///   `invalid_token` / `verifier_unavailable`;
    /// - **the credential verifies but is not a CUSTOMER** — `role_not_customer`. An ADMIN /
    ///   RESTAURANT / RESTAURANT_ACCOUNT / RIDER token on the open path is anonymous, NOT elevated:
    ///   "role = path" (ADR-0047) is the whole reason staff must present their token to their own
    ///   path, and elevating here would turn a dead claim leg into privilege escalation on the one
    ///   path anyone can reach. Staff lose nothing — their surfaces talk to `/restaurant`,
    ///   `/rider`, `/admin`, which are unchanged.
    /// - **the CUSTOMER token carries no `captain_customer_id`** — `claim_absent` (reviewer S3).
    ///   This is the KNOWN LIMITATION recorded on [`Principal`]: a token minted BEFORE the claim
    ///   stamp, i.e. every signed-in customer for one token lifetime after rollout. Such a caller
    ///   resolves to `ReadScope::Public` whichever way it is spelled — no cart, no ownership match,
    ///   no leg — so it is a degrade, and it is counted as one HERE rather than falling through to
    ///   `read_scope`'s `read_authorization_bridge_unresolved_total`. That counter's contract says
    ///   it is *"a provisioning gap or staleness — never ordinary user denial"*, and a normal
    ///   rollout bumping it on EVERY storefront GraphQL request would read to an operator as an
    ///   incident: exactly the misreading `public_credential_degraded_total` exists to prevent.
    ///   `bridge_unresolved` keeps its meaning — an authenticated caller on a ROLE path who is
    ///   consequently DENIED something — which is why the counter is re-routed rather than the
    ///   contract text widened.
    ///
    /// A request with NO credential at all short-circuits before any I/O: anonymous browsing costs
    /// exactly what it cost before this change (no JWKS fetch, no verification).
    async fn public_principal(&self, headers: &HeaderMap) -> Principal {
        let Some(session_token) = token(headers) else {
            return Principal::anonymous();
        };
        let claims = match self.verify(session_token).await {
            Ok(claims) => claims,
            Err(e) => {
                telemetry::meters::read_authorization::public_credential_degraded(match e {
                    AuthError::Unavailable => "verifier_unavailable",
                    _ => "invalid_token",
                });
                return Principal::anonymous();
            }
        };
        let granted = claims
            .app_metadata
            .captain_role
            .as_deref()
            .map(parse_role)
            .unwrap_or(RequestRole::Customer);
        if granted != RequestRole::Customer {
            telemetry::meters::read_authorization::public_credential_degraded("role_not_customer");
            return Principal::anonymous();
        }
        // No domain claim (or a malformed one) = nothing this path can act on: serve anonymous and
        // count it here, so the pre-claim-stamp window is a visible degrade rather than a
        // provisioning-gap alarm on every storefront request.
        let Some(customer_id) = claim_uuid(&claims.app_metadata.captain_customer_id) else {
            telemetry::meters::read_authorization::public_credential_degraded("claim_absent");
            return Principal::anonymous();
        };
        Principal::public_customer(claims.sub, customer_id)
    }

    /// Verify a JWT's signature (asymmetric, key + algorithm from the JWKS) and reserved claims.
    async fn verify(&self, token: &str) -> Result<Claims, AuthError> {
        let header = decode_header(token).map_err(|_| AuthError::Unauthorized)?;
        let kid = header.kid.ok_or(AuthError::Unauthorized)?;
        let jwk = self.key_for(&kid).await?;
        let alg = asymmetric_alg(&jwk, header.alg).ok_or(AuthError::Unauthorized)?;
        let key = DecodingKey::from_jwk(&jwk).map_err(|_| AuthError::Unauthorized)?;

        let mut validation = Validation::new(alg);
        validation.set_audience(&[SUPABASE_AUDIENCE]);
        if let Some(iss) = &self.issuer {
            validation.set_issuer(&[iss]);
        }
        // `exp` is validated by default.
        decode::<Claims>(token, &key, &validation)
            .map(|data| data.claims)
            .map_err(|_| AuthError::Unauthorized)
    }

    /// Find the JWK for `kid`, refreshing the cache if stale or if the key is unknown (rotation).
    ///
    /// **Serve-stale-on-refresh-failure**: if the set is past its TTL but the refresh fails (a transient
    /// JWKS outage), keep using the cached keys rather than locking everyone out — signing keys rotate
    /// rarely, so the TTL is a freshness hint, not a hard expiry. A genuinely unknown `kid` still forces a
    /// refetch and **fails closed** if it can't be resolved.
    async fn key_for(&self, kid: &str) -> Result<Jwk, AuthError> {
        if self.stale().await {
            if let Err(e) = self.refresh().await {
                // Fail closed only when there is nothing cached to fall back to (cold cache).
                if self.cache.read().await.is_none() {
                    return Err(e);
                }
                // Otherwise carry on with the stale-but-present keys.
            }
        }
        if let Some(jwk) = self.lookup(kid).await {
            return Ok(jwk);
        }
        // Unknown kid (e.g. a just-rotated key): force one refetch to absorb rotation; fail closed if it
        // still can't be resolved.
        self.refresh().await?;
        self.lookup(kid).await.ok_or(AuthError::Unauthorized)
    }

    async fn stale(&self) -> bool {
        match &*self.cache.read().await {
            Some(c) => c.fetched.elapsed() > JWKS_TTL,
            None => true,
        }
    }

    async fn lookup(&self, kid: &str) -> Option<Jwk> {
        self.cache.read().await.as_ref().and_then(|c| c.set.find(kid).cloned())
    }

    async fn refresh(&self) -> Result<(), AuthError> {
        let url = self.jwks_url.as_deref().ok_or(AuthError::Unavailable)?;
        let set = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| AuthError::Unavailable)?
            .json::<JwkSet>()
            .await
            .map_err(|_| AuthError::Unavailable)?;
        *self.cache.write().await = Some(CachedJwks { set, fetched: Instant::now() });
        Ok(())
    }
}

impl AuthContext {
    /// True when `presented` matches a configured EXTERNAL service token, compared in constant time.
    fn external_key_valid(&self, presented: &str) -> bool {
        self.external_tokens.iter().any(|t| ct_eq(t.as_bytes(), presented.as_bytes()))
    }
}

/// The JWKS fetch client — **bounded**, deliberately (#469). Key refresh is a once-an-hour call on
/// a request's critical path, and since the open path started verifying credentials that path is
/// the STOREFRONT's. An unreachable-but-not-refusing JWKS host (a black-holed TCP connection) would
/// otherwise hang every cookie-carrying public request for as long as the socket stays open; the
/// timeout converts that into the honest `verifier_unavailable` degrade to anonymous, in seconds.
fn jwks_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(JWKS_FETCH_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Constant-time byte equality — no early return on the first differing byte, so a matching-prefix key
/// can't be discovered by timing. (Length is allowed to differ observably, as is standard for API keys.)
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Extract the `Authorization: Bearer <token>` value.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// The auth cookie name minted by `POST /auth/session` (#112, PROP-20260724-150500) — the httpOnly
/// carrier of the provider access JWT. MUST stay in sync with the value the auth routes set.
pub const AUTH_COOKIE: &str = "captain_auth";

/// The verified session token, from the `Authorization: Bearer` header OR the `captain_auth` cookie
/// (#112). The header wins when both are present (an explicit bearer is a deliberate override); the
/// cookie is the browser's carrier — same-origin `fetch`, the WS upgrade, and SSR all send it
/// automatically, which is exactly why one fallback here lights all three (the issue's core move).
fn token(headers: &HeaderMap) -> Option<&str> {
    bearer(headers).or_else(|| cookie_value(headers, AUTH_COOKIE))
}

/// Read one cookie value from the `Cookie` header (`a=1; b=2`). Borrowed slice — no allocation.
fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(axum::http::header::COOKIE).and_then(|v| v.to_str().ok()).and_then(|raw| {
        raw.split(';').map(str::trim).find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            (k.trim() == name).then(|| v.trim()).filter(|t| !t.is_empty())
        })
    })
}

/// Resolve the algorithm from the matched JWK (falling back to the header only for asymmetric families).
/// Restricting to asymmetric algorithms defeats `alg`-confusion (no HS* downgrade against a public key).
fn asymmetric_alg(jwk: &Jwk, header_alg: Algorithm) -> Option<Algorithm> {
    let from_jwk = jwk.common.key_algorithm.and_then(key_alg_to_alg);
    let alg = from_jwk.unwrap_or(header_alg);
    is_asymmetric(alg).then_some(alg)
}

fn key_alg_to_alg(k: KeyAlgorithm) -> Option<Algorithm> {
    Some(match k {
        KeyAlgorithm::RS256 => Algorithm::RS256,
        KeyAlgorithm::RS384 => Algorithm::RS384,
        KeyAlgorithm::RS512 => Algorithm::RS512,
        KeyAlgorithm::ES256 => Algorithm::ES256,
        KeyAlgorithm::ES384 => Algorithm::ES384,
        KeyAlgorithm::EdDSA => Algorithm::EdDSA,
        KeyAlgorithm::PS256 => Algorithm::PS256,
        KeyAlgorithm::PS384 => Algorithm::PS384,
        KeyAlgorithm::PS512 => Algorithm::PS512,
        _ => return None,
    })
}

fn is_asymmetric(alg: Algorithm) -> bool {
    !matches!(alg, Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512)
}

/// Map a `captain_role` claim to a role. Unknown/absent ⇒ CUSTOMER (least-privilege authenticated baseline).
fn parse_role(s: &str) -> RequestRole {
    match s.trim().to_ascii_uppercase().as_str() {
        "ADMIN" => RequestRole::Admin,
        "CUSTOMER" => RequestRole::Customer,
        "RESTAURANT" => RequestRole::Restaurant,
        "RESTAURANT_ACCOUNT" => RequestRole::RestaurantAccount,
        "RIDER" => RequestRole::Rider,
        "EXTERNAL" => RequestRole::External,
        _ => RequestRole::Customer,
    }
}

/// A caller granted `granted` may act on the `path_role` path. Strict equality: an ADMIN token must use
/// `/admin`, not `/customer`. `Public` is handled before this (open), so it never reaches here.
fn role_permitted(path_role: RequestRole, granted: RequestRole) -> bool {
    path_role == granted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_maps_to_role_with_customer_default() {
        assert_eq!(parse_role("ADMIN"), RequestRole::Admin);
        assert_eq!(parse_role("admin"), RequestRole::Admin);
        assert_eq!(parse_role("RESTAURANT_ACCOUNT"), RequestRole::RestaurantAccount);
        assert_eq!(parse_role("EXTERNAL"), RequestRole::External);
        assert_eq!(parse_role("nonsense"), RequestRole::Customer);
    }

    #[test]
    fn role_gate_is_strict_equality() {
        assert!(role_permitted(RequestRole::Admin, RequestRole::Admin));
        assert!(role_permitted(RequestRole::Customer, RequestRole::Customer));
        // An ADMIN token cannot use the /customer path, and vice-versa.
        assert!(!role_permitted(RequestRole::Customer, RequestRole::Admin));
        assert!(!role_permitted(RequestRole::Admin, RequestRole::Customer));
        assert!(!role_permitted(RequestRole::Rider, RequestRole::Restaurant));
    }

    #[test]
    fn hs_algorithms_are_rejected_asymmetric_kept() {
        assert!(is_asymmetric(Algorithm::RS256));
        assert!(is_asymmetric(Algorithm::ES256));
        assert!(!is_asymmetric(Algorithm::HS256));
    }

    #[test]
    fn bearer_is_parsed_case_insensitively() {
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, "Bearer abc.def.ghi".parse().unwrap());
        assert_eq!(bearer(&h), Some("abc.def.ghi"));
        h.insert(AUTHORIZATION, "bearer  xyz ".parse().unwrap());
        assert_eq!(bearer(&h), Some("xyz"));
        h.insert(AUTHORIZATION, "Basic zzz".parse().unwrap());
        assert_eq!(bearer(&h), None);
    }

    #[test]
    fn token_falls_back_to_the_auth_cookie_and_the_header_wins() {
        // #112: the cookie carries the session when no bearer is present — the one seam that lights
        // HTTP, the WS upgrade and SSR (all send cookies automatically).
        let mut h = HeaderMap::new();
        h.insert(axum::http::header::COOKIE, "other=1; captain_auth=jwt.from.cookie; x=2".parse().unwrap());
        assert_eq!(token(&h), Some("jwt.from.cookie"));
        // An explicit bearer overrides the cookie (deliberate override).
        h.insert(AUTHORIZATION, "Bearer jwt.from.header".parse().unwrap());
        assert_eq!(token(&h), Some("jwt.from.header"));
        // Neither present → none.
        let empty = HeaderMap::new();
        assert_eq!(token(&empty), None);
        // A cookie header without our cookie → none.
        let mut h2 = HeaderMap::new();
        h2.insert(axum::http::header::COOKIE, "session=abc".parse().unwrap());
        assert_eq!(token(&h2), None);
    }

    /// A single-key JWKS (dummy RSA material — enough to parse + `find(kid)`, not to verify a signature).
    fn test_set() -> JwkSet {
        serde_json::from_str(
            r#"{"keys":[{"kty":"RSA","use":"sig","kid":"test-kid","alg":"RS256","n":"0vx7agoebGcQSuuPiLJXZptN","e":"AQAB"}]}"#,
        )
        .expect("parse test JWKS")
    }

    /// An `Instant` far enough in the past to be stale (falls back to `now` on a very-low-uptime host,
    /// which still keeps the assertions below valid).
    fn stale_instant() -> Instant {
        Instant::now().checked_sub(JWKS_TTL * 2).unwrap_or_else(Instant::now)
    }

    fn ctx_with_cache(set: JwkSet, fetched: Instant) -> AuthContext {
        AuthContext {
            jwks_url: None, // any refresh() therefore fails — proves we never hit the network on a cache hit
            issuer: None,
            external_tokens: Vec::new(),
            http: reqwest::Client::new(),
            cache: RwLock::new(Some(CachedJwks { set, fetched })),
        }
    }

    fn ctx_with_external(tokens: &[&str]) -> AuthContext {
        AuthContext {
            jwks_url: None,
            issuer: None,
            external_tokens: tokens.iter().map(|t| t.to_string()).collect(),
            http: reqwest::Client::new(),
            cache: RwLock::new(None),
        }
    }

    #[test]
    fn from_config_uses_its_arguments_not_env() {
        // Regression guard (prod-smoke L4 503 "auth unavailable", 2026-08-01): SUPABASE_JWKS_URL /
        // SUPABASE_URL are non-secret BAKED config (ADR-20260729-020000) — present in the resolved
        // `Config`, absent from the deployed service's env. The verifier must take them from that
        // resolved config, never from `std::env` directly, or every authenticated path fails closed.
        let ctx = AuthContext::from_config(
            "https://example.test/jwks.json".into(),
            "https://proj.supabase.co/".into(),
        );
        assert_eq!(ctx.jwks_url.as_deref(), Some("https://example.test/jwks.json"));
        // issuer is derived from supabase_url, trailing slash trimmed.
        assert_eq!(ctx.issuer.as_deref(), Some("https://proj.supabase.co/auth/v1"));

        // Empty resolved values (e.g. profile with no baked value and no env) → fail closed, no issuer.
        let empty = AuthContext::from_config(String::new(), String::new());
        assert!(empty.jwks_url.is_none(), "empty JWKS URL must yield no verifier (fail closed)");
        assert!(empty.issuer.is_none());
    }

    #[test]
    fn ct_eq_matches_only_identical_bytes() {
        assert!(ct_eq(b"s3cret-key", b"s3cret-key"));
        assert!(!ct_eq(b"s3cret-key", b"s3cret-keZ"));
        assert!(!ct_eq(b"s3cret-key", b"s3cret-ke")); // differing length
    }

    #[tokio::test]
    async fn external_service_key_authorizes_only_the_external_path() {
        let ctx = ctx_with_external(&["s3cret-key", "second-key"]);
        let mut ok = HeaderMap::new();
        ok.insert("x-external-api-key", "second-key".parse().unwrap());
        assert!(ctx.authorize(RequestRole::External, &ok).await.is_ok(), "valid key must authorize");

        let mut bad = HeaderMap::new();
        bad.insert("x-external-api-key", "wrong".parse().unwrap());
        assert!(ctx.authorize(RequestRole::External, &bad).await.is_err(), "wrong key must fail");

        // The external key is ignored on other paths (they still require a JWT, absent here → 401).
        assert!(ctx.authorize(RequestRole::Admin, &ok).await.is_err(), "external key must not open /admin");
        // /public stays open regardless.
        assert!(ctx.authorize(RequestRole::Public, &HeaderMap::new()).await.is_ok());
    }

    #[tokio::test]
    async fn serve_stale_returns_a_cached_key_when_refresh_fails() {
        // Stale cache + failing refresh (no JWKS URL): a known kid is still served from the cached set.
        let ctx = ctx_with_cache(test_set(), stale_instant());
        assert!(ctx.key_for("test-kid").await.is_ok(), "present key must survive a failed refresh");
    }

    #[tokio::test]
    async fn unknown_kid_still_fails_closed_when_refresh_fails() {
        // A kid we've never cached cannot be served from stale data and cannot be fetched → rejected.
        let ctx = ctx_with_cache(test_set(), stale_instant());
        assert!(ctx.key_for("rotated-unknown-kid").await.is_err(), "unknown key must fail closed");
    }

    /// TEST-ONLY ES256 keypair (generated for this test file, never a deployed key): the PEM signs,
    /// [`signing_set`] is its public point as a JWKS. Unlike [`test_set`]'s dummy material this
    /// key VERIFIES — it exists so [`cookie_delivered_jwt_yields_the_customer_principal_and_scope`]
    /// can exercise `authorize()` end to end (signature check included), the one link no pure test
    /// reaches (#437, beck's mandatory test: a claim→field transposition at the `Principal`
    /// construction passes every other test in this crate).
    const TEST_EC_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgTCTNdGfegiVKVsm+
vZXPOa4xJAt5OT8zMSblfCEwtW2hRANCAARto0Dk75fxl2IyLx89vwvjUWkJAb/p
5bKnk8sNetDUBHLVIGpXoxBRFJVNSeDN6QB9IHl6rqDLaZR4iqLatScL
-----END PRIVATE KEY-----
";

    fn signing_set() -> JwkSet {
        serde_json::from_str(
            r#"{"keys":[{"kty":"EC","crv":"P-256","use":"sig","kid":"captain-test-es256","alg":"ES256","x":"baNA5O-X8ZdiMi8fPb8L41FpCQG_6eWyp5PLDXrQ1AQ","y":"ctUgalejEFEUlU1J4M3pAH0geXquoMtplHiKotq1Jws"}]}"#,
        )
        .expect("parse signing JWKS")
    }

    /// Sign a Supabase-shaped customer JWT: `sub` is the AUTH subject (deliberately a different
    /// uuid from the claim — an implementation deriving identity from `sub` cannot pass), the
    /// domain identity is `app_metadata.captain_customer_id` (#433/#437).
    fn signed_customer_jwt(sub: &str, customer_id: uuid::Uuid) -> String {
        signed_jwt(
            sub,
            serde_json::json!({
                "captain_role": "CUSTOMER",
                "captain_customer_id": customer_id.to_string(),
            }),
            3600,
        )
    }

    /// A Supabase-shaped JWT with an arbitrary `app_metadata` and lifetime, signed by the test key.
    /// `ttl_secs` is signed: a NEGATIVE value produces an EXPIRED token (the stale-cookie case,
    /// which is the common one on the open path, not an exotic one).
    fn signed_jwt(sub: &str, app_metadata: serde_json::Value, ttl_secs: i64) -> String {
        let mut header = jsonwebtoken::Header::new(Algorithm::ES256);
        header.kid = Some("captain-test-es256".into());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_secs() as i64;
        let claims = serde_json::json!({
            "sub": sub,
            "aud": SUPABASE_AUDIENCE,
            "exp": now + ttl_secs,
            "app_metadata": app_metadata,
        });
        let key = jsonwebtoken::EncodingKey::from_ec_pem(TEST_EC_PRIVATE_KEY_PEM.as_bytes())
            .expect("test EC key parses");
        jsonwebtoken::encode(&header, &claims, &key).expect("sign test JWT")
    }

    /// The storefront's own request shape: a cookie-delivered credential, no Authorization header.
    fn cookie_headers(jwt: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(axum::http::header::COOKIE, format!("captain_auth={jwt}").parse().unwrap());
        h
    }

    /// #469 half 1, the fix itself at the auth seam: the OPEN path READS the credential the
    /// storefront actually sends (`web::router` pins the customer surfaces to `Role::Public`), so a
    /// signed-in customer arrives as `ReadScope::Customer` and `cart.current`'s claim leg can fire.
    /// Before this, `/public` returned `Principal::anonymous()` without reading any credential, and
    /// leg 1 was unreachable from a browser.
    #[tokio::test]
    async fn the_open_path_reads_the_customer_credential_it_used_to_ignore() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0xA0).to_string();
        let customer = uuid::Uuid::from_u128(0x469);

        let principal = ctx
            .authorize(RequestRole::Public, &cookie_headers(&signed_customer_jwt(&sub, customer)))
            .await
            .expect("the open path never fails");
        assert_eq!(principal.customer_id, Some(customer), "the verified claim IS the identity");
        assert_eq!(principal.user_id.as_deref(), Some(sub.as_str()), "sub stays the auth subject");
        assert_eq!(
            read_scope(&principal),
            application::queries::ReadScope::Customer(domain::generated::scalars::CustomerId(
                customer
            )),
            "leg 1 of cart.current is reachable from the storefront's own request shape"
        );
    }

    /// #469 half 1, the escalation pin (mob: testing lens 4a + API lens 3, reached independently).
    /// Once `/public` reads credentials, an ADMIN / RESTAURANT / RESTAURANT_ACCOUNT / RIDER token
    /// presented there must degrade to ANONYMOUS — never yield `ReadScope::Admin`/`Restaurant`/
    /// `Rider`, and never 401. Elevating here would convert a dead claim leg into privilege
    /// escalation on the one path anyone can reach, and would silently widen every role-omitted
    /// read (an omitted `roles` key is open to every role path — specs/common/api.yaml).
    #[tokio::test]
    async fn a_staff_token_on_the_open_path_stays_anonymous() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0xA1).to_string();
        let tenant = uuid::Uuid::from_u128(0xBEEF);

        for role in ["ADMIN", "RESTAURANT", "RESTAURANT_ACCOUNT", "RIDER", "EXTERNAL"] {
            // The token carries EVERY tenant claim at once: if the open path copied claims
            // wholesale instead of constructing a customer-only principal, this is what would leak.
            let jwt = signed_jwt(
                &sub,
                serde_json::json!({
                    "captain_role": role,
                    "captain_restaurant_id": tenant.to_string(),
                    "captain_restaurant_account_id": tenant.to_string(),
                    "captain_rider_id": tenant.to_string(),
                    "captain_customer_id": tenant.to_string(),
                }),
                3600,
            );
            let principal = ctx
                .authorize(RequestRole::Public, &cookie_headers(&jwt))
                .await
                .unwrap_or_else(|_| panic!("{role}: the open path must never refuse"));
            assert_eq!(principal.role, RequestRole::Public, "{role}: not elevated");
            assert_eq!(principal.user_id, None, "{role}: no identity on the open path");
            assert_eq!(principal.restaurant_id, None, "{role}: no tenant claim survives");
            assert_eq!(principal.restaurant_account_id, None, "{role}");
            assert_eq!(principal.rider_id, None, "{role}");
            assert_eq!(principal.customer_id, None, "{role}");
            assert_eq!(
                read_scope(&principal),
                application::queries::ReadScope::Public,
                "{role}: a staff token on /public reads exactly what an anonymous browser reads"
            );
        }
    }

    /// #469, reviewer S3: a verified CUSTOMER token with NO `captain_customer_id` — the
    /// pre-claim-stamp window, which is EVERY signed-in customer for one token lifetime after
    /// rollout — degrades to anonymous and is counted as `claim_absent`, rather than becoming a
    /// `role: Customer, customer_id: None` principal.
    ///
    /// The assertion that carries the point is `role == Public`: `read_scope`'s
    /// `unresolved("CUSTOMER")` arm — which bumps `read_authorization_bridge_unresolved_total`, a
    /// counter whose contract says *"never ordinary user denial"* — is reachable ONLY through
    /// `role: Customer`. A principal that never claims that role cannot reach it, so a normal
    /// rollout cannot bump a provisioning-gap counter on every storefront GraphQL request. A
    /// MALFORMED claim is the same state by design (fail closed, indistinguishable from absent).
    #[tokio::test]
    async fn a_customer_token_without_its_claim_degrades_instead_of_alarming() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0xA3).to_string();

        for (case, app_metadata) in [
            ("claim absent", serde_json::json!({ "captain_role": "CUSTOMER" })),
            (
                "claim malformed",
                serde_json::json!({ "captain_role": "CUSTOMER", "captain_customer_id": "not-a-uuid" }),
            ),
        ] {
            let jwt = signed_jwt(&sub, app_metadata, 3600);
            let principal = ctx
                .authorize(RequestRole::Public, &cookie_headers(&jwt))
                .await
                .unwrap_or_else(|_| panic!("{case}: the open path must never refuse"));
            assert_eq!(
                principal.role,
                RequestRole::Public,
                "{case}: an unusable claim is a DEGRADE, not a claimless Customer principal"
            );
            assert_eq!(principal.customer_id, None, "{case}");
            assert_eq!(
                read_scope(&principal),
                application::queries::ReadScope::Public,
                "{case}: the same scope either way -- so the only difference would have been the alarm"
            );
        }
    }

    /// #469, reviewer N1: `parse_role`'s catch-all (`_ => CUSTOMER`, the least-privilege
    /// authenticated baseline) is reached by an UNKNOWN role string and by an ABSENT `captain_role`
    /// — two branches the loop in `a_staff_token_on_the_open_path_stays_anonymous` cannot cover,
    /// because it enumerates the five explicit non-CUSTOMER roles. Pinned so that an edit to that
    /// arm cannot silently change what the open path grants.
    ///
    /// The documented outcome: such a caller is treated as a CUSTOMER and gets THEIR OWN customer
    /// scope — never a tenant one, even though the token carries every tenant claim.
    #[tokio::test]
    async fn an_unknown_or_absent_role_claim_is_the_customer_baseline_never_a_tenant_scope() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0xA4).to_string();
        let customer = uuid::Uuid::from_u128(0x469C);
        let tenant = uuid::Uuid::from_u128(0xBEEF);

        let with_claims = |role: Option<&str>| {
            let mut meta = serde_json::json!({
                "captain_customer_id": customer.to_string(),
                "captain_restaurant_id": tenant.to_string(),
                "captain_restaurant_account_id": tenant.to_string(),
                "captain_rider_id": tenant.to_string(),
            });
            if let Some(role) = role {
                meta["captain_role"] = serde_json::json!(role);
            }
            meta
        };

        for (case, role) in
            [("empty", Some("")), ("unknown", Some("SUPERADMIN")), ("absent", None)]
        {
            let jwt = signed_jwt(&sub, with_claims(role), 3600);
            let principal = ctx
                .authorize(RequestRole::Public, &cookie_headers(&jwt))
                .await
                .unwrap_or_else(|_| panic!("{case}: the open path must never refuse"));
            assert_eq!(
                read_scope(&principal),
                application::queries::ReadScope::Customer(
                    domain::generated::scalars::CustomerId(customer)
                ),
                "{case}: the catch-all is the CUSTOMER baseline -- the caller's OWN scope"
            );
            assert_eq!(principal.restaurant_id, None, "{case}: no tenant claim survives");
            assert_eq!(principal.restaurant_account_id, None, "{case}");
            assert_eq!(principal.rider_id, None, "{case}");
        }
    }

    /// #469 half 1, the availability pin (mob: testing lens 4b + API lens 3). A stale cookie is the
    /// COMMON case and a JWKS outage is a platform one; neither may take anonymous browsing down.
    /// Expired, tampered, unsigned-by-us and unverifiable-because-no-JWKS all serve the anonymous
    /// view — 200, never 401/503.
    #[tokio::test]
    async fn an_unverifiable_credential_on_the_open_path_degrades_never_refuses() {
        let ctx = ctx_with_cache(signing_set(), Instant::now());
        let sub = uuid::Uuid::from_u128(0xA2).to_string();
        let customer = uuid::Uuid::from_u128(0x469);

        // 1. EXPIRED — the stale cookie of a customer who left the tab open overnight.
        let expired = signed_jwt(
            &sub,
            serde_json::json!({ "captain_role": "CUSTOMER", "captain_customer_id": customer.to_string() }),
            -3600,
        );
        // 2. TAMPERED — a flipped payload byte, i.e. a forged claim.
        let valid = signed_customer_jwt(&sub, customer);
        let tampered = {
            let mut parts: Vec<String> = valid.split('.').map(str::to_string).collect();
            let p = &mut parts[1];
            let last = p.pop().expect("payload non-empty");
            p.push(if last == 'A' { 'B' } else { 'A' });
            parts.join(".")
        };
        for (case, jwt) in [("expired", expired), ("tampered", tampered), ("garbage", "not.a.jwt".into())] {
            let principal = ctx
                .authorize(RequestRole::Public, &cookie_headers(&jwt))
                .await
                .unwrap_or_else(|_| panic!("{case}: /public must serve 200 anonymous, never 401"));
            assert_eq!(principal.role, RequestRole::Public, "{case}");
            assert_eq!(principal.customer_id, None, "{case}");
        }

        // 3. The VERIFIER ITSELF is unavailable (no JWKS configured, empty cache — a cold instance
        //    during a Supabase outage): still anonymous, still 200. `/public` worked with no JWKS
        //    at all before this change and must keep working.
        let no_verifier = AuthContext {
            jwks_url: None,
            issuer: None,
            external_tokens: Vec::new(),
            http: jwks_client(),
            cache: RwLock::new(None),
        };
        let principal = no_verifier
            .authorize(RequestRole::Public, &cookie_headers(&valid))
            .await
            .expect("a JWKS outage must not take anonymous browsing down");
        assert_eq!(principal.role, RequestRole::Public);
        assert_eq!(principal.customer_id, None);

        // 4. NO credential at all: anonymous without touching the verifier (the cost of anonymous
        //    browsing is unchanged — `jwks_url: None` here means any fetch attempt would fail, and
        //    the assertion above it would too).
        let anonymous = no_verifier
            .authorize(RequestRole::Public, &HeaderMap::new())
            .await
            .expect("no credential is the ordinary anonymous request");
        assert_eq!(anonymous.role, RequestRole::Public);
    }

    /// The end-to-end link every pure test stops short of (#437): a REAL signature verified
    /// against a seeded JWKS, the token delivered ONLY via the `captain_auth` cookie (the
    /// storefront's one credential — no Authorization header exists on the request), and the
    /// verified claim landing in `Principal.customer_id` → `ReadScope::Customer`. A field
    /// transposition in `authorize()`'s Principal construction (customer claim into `rider_id`)
    /// is caught HERE and nowhere else.
    #[tokio::test]
    async fn cookie_delivered_jwt_yields_the_customer_principal_and_scope() {
        let ctx = ctx_with_cache(signing_set(), Instant::now()); // fresh cache: no network, ever
        let sub = uuid::Uuid::from_u128(0xA0).to_string();
        let customer = uuid::Uuid::from_u128(0x437);
        let jwt = signed_customer_jwt(&sub, customer);

        let mut h = HeaderMap::new();
        h.insert(axum::http::header::COOKIE, format!("captain_auth={jwt}").parse().unwrap());
        assert!(h.get(AUTHORIZATION).is_none(), "cookie-only delivery: no bearer on this request");

        let principal =
            ctx.authorize(RequestRole::Customer, &h).await.expect("cookie-carried JWT authorizes");
        assert_eq!(principal.customer_id, Some(customer), "the verified claim IS the identity");
        assert_eq!(principal.rider_id, None, "no cross-field leakage (the transposition trap)");
        assert_eq!(principal.restaurant_id, None);
        assert_eq!(principal.restaurant_account_id, None);
        assert_eq!(principal.user_id.as_deref(), Some(sub.as_str()), "sub stays the auth subject");
        assert_eq!(
            read_scope(&principal),
            application::queries::ReadScope::Customer(domain::generated::scalars::CustomerId(customer)),
            "the scope every #437 tracking read is filtered by"
        );

        // The signature is REALLY checked: tamper with the last payload byte → Unauthorized.
        let mut parts: Vec<String> = jwt.split('.').map(str::to_string).collect();
        let flipped = {
            let p = &mut parts[1];
            let last = p.pop().expect("payload non-empty");
            p.push(if last == 'A' { 'B' } else { 'A' });
            parts.join(".")
        };
        let mut tampered = HeaderMap::new();
        tampered
            .insert(axum::http::header::COOKIE, format!("captain_auth={flipped}").parse().unwrap());
        assert!(
            ctx.authorize(RequestRole::Customer, &tampered).await.is_err(),
            "a tampered payload must be rejected by signature verification"
        );
    }
}

/// A `captain_*` claim parsed as a domain uuid. Malformed values yield `None` — fail closed,
/// indistinguishable from an absent claim by design (an attacker-shaped string must never widen
/// into an identity; `read_scope` then denies and counts it as unresolved).
fn claim_uuid(v: &Option<String>) -> Option<uuid::Uuid> {
    v.as_deref().and_then(|s| uuid::Uuid::parse_str(s).ok())
}

/// Resolve a verified [`Principal`] into the application's [`application::queries::ReadScope`] —
/// a PURE function of the token's verified claims (#433, ADR-20260809-050000 CARD-11: the
/// login-to-domain bridge lives in JWT claims for EVERY role; product-owner correction on #430).
///
/// No per-request lookup, no database, no async: `sub` is NEVER an identity — a customer or rider
/// token whose `captain_*` claim is absent (or malformed) fails closed to Public, and the
/// `read_authorization_bridge_unresolved_total{role}` counter now means exactly one thing: an
/// authenticated caller whose token carries no domain binding (a provisioning gap or pre-refresh
/// staleness — never ordinary user denial, never a DB outage).
///
/// The #430 mechanisms this replaced: the per-request `customers.by_auth_ref` bridge and the
/// rider `sub`-parsed-as-uuid placeholder. `by_auth_ref` REMAINS the customer identity mechanism
/// at the write-side seams (the mailbox `resolve_actor`, the generated mutation edge bridges) —
/// a named follow-up on #432, envelope-shape territory, not silently claimed here.
pub fn read_scope(principal: &Principal) -> application::queries::ReadScope {
    use application::queries::ReadScope;
    use domain::generated::scalars::{CustomerId, RestaurantAccountId, RestaurantId, RiderId};

    let unresolved = |role: &str| {
        telemetry::meters::read_authorization::bridge_unresolved(role);
        ReadScope::Public
    };
    match principal.role {
        RequestRole::Admin => ReadScope::Admin,
        RequestRole::Restaurant => match principal.restaurant_id {
            Some(id) => ReadScope::Restaurant(RestaurantId(id)),
            None => unresolved("RESTAURANT"),
        },
        RequestRole::RestaurantAccount => match principal.restaurant_account_id {
            Some(id) => ReadScope::RestaurantAccount(RestaurantAccountId(id)),
            None => unresolved("RESTAURANT_ACCOUNT"),
        },
        RequestRole::Customer => match principal.customer_id {
            Some(id) => ReadScope::Customer(CustomerId(id)),
            None => unresolved("CUSTOMER"),
        },
        RequestRole::Rider => match principal.rider_id {
            Some(id) => ReadScope::Rider(RiderId(id)),
            None => unresolved("RIDER"),
        },
        RequestRole::Public | RequestRole::External => ReadScope::Public,
    }
}

/// [`read_scope`] under the contract's `auth.read_scope` span (ONE per request), stamped with the
/// request's correlation id. This is the transport entry point; the pure function above is the
/// logic.
///
/// The id is PASSED IN, never minted here (#451 Phase 2b): reads carry no command envelope, so the
/// server mints `request.correlation_id` once at the transport boundary
/// ([`crate::graphql::session::RequestCorrelationId`]) and every read-path span of that request —
/// `auth.read_scope` here, `cart.price` at the pricing seam — records the SAME value. Minting one
/// per span made a single request emit several unrelated ids, which is the attribute present and
/// the correlation absent.
pub fn resolve_read_scope(
    principal: &Principal,
    correlation_id: crate::graphql::session::RequestCorrelationId,
) -> application::queries::ReadScope {
    let span = telemetry::spans::auth_read_scope(&format!("{:?}", principal.role));
    span.record("business.correlation_id", correlation_id.0.to_string().as_str());
    let scope = span.in_scope(|| read_scope(principal));
    telemetry::spans::record_bridge_resolved(
        &span,
        !matches!(scope, application::queries::ReadScope::Public)
            || principal.role == RequestRole::Public
            || principal.role == RequestRole::External,
    );
    scope
}

#[cfg(test)]
mod read_scope_tests {
    use application::queries::ReadScope;
    use domain::generated::scalars::{CustomerId, RestaurantAccountId, RestaurantId, RiderId};

    use super::*;

    fn principal(role: RequestRole, sub: Option<&str>) -> Principal {
        Principal {
            user_id: sub.map(str::to_string),
            role,
            restaurant_id: None,
            restaurant_account_id: None,
            customer_id: None,
            rider_id: None,
        }
    }

    /// The pure claims function, per role. The load-bearing data shape (beck): `sub` and the claim
    /// are DIFFERENT uuids — an implementation that still derives identity from `sub` fails these
    /// assertions instead of passing by coincidence. Seen RED by re-planting #430's fallbacks
    /// (customer via `user_id`, rider via `sub`-parse): the absent-claim arms below went red with
    /// "sub is never an identity", green restored on removal.
    #[test]
    fn read_scope_is_a_pure_claims_function() {
        let sub = uuid::Uuid::from_u128(1).to_string();

        // Claim present -> the DOMAIN id, verbatim — never the subject.
        let customer_claim = uuid::Uuid::from_u128(2);
        let mut p = principal(RequestRole::Customer, Some(&sub));
        p.customer_id = Some(customer_claim);
        assert_eq!(read_scope(&p), ReadScope::Customer(CustomerId(customer_claim)));

        let rider_claim = uuid::Uuid::from_u128(4);
        let mut p = principal(RequestRole::Rider, Some(&sub));
        p.rider_id = Some(rider_claim);
        assert_eq!(read_scope(&p), ReadScope::Rider(RiderId(rider_claim)));

        let rid = uuid::Uuid::from_u128(11);
        let mut p = principal(RequestRole::Restaurant, Some("s"));
        p.restaurant_id = Some(rid);
        assert_eq!(read_scope(&p), ReadScope::Restaurant(RestaurantId(rid)));
        let mut p = principal(RequestRole::RestaurantAccount, Some("s"));
        p.restaurant_account_id = Some(rid);
        assert_eq!(read_scope(&p), ReadScope::RestaurantAccount(RestaurantAccountId(rid)));

        // Claim ABSENT -> Public, even with a perfectly parseable sub: sub is never an identity.
        assert_eq!(
            read_scope(&principal(RequestRole::Customer, Some(&sub))),
            ReadScope::Public,
            "sub is never an identity (customer)"
        );
        assert_eq!(
            read_scope(&principal(RequestRole::Rider, Some(&sub))),
            ReadScope::Public,
            "sub is never an identity (rider — #430's placeholder must stay dead)"
        );
        assert_eq!(read_scope(&principal(RequestRole::Restaurant, Some("s"))), ReadScope::Public);
        assert_eq!(
            read_scope(&principal(RequestRole::RestaurantAccount, Some("s"))),
            ReadScope::Public
        );

        // Role decisions, no claims involved.
        assert_eq!(read_scope(&principal(RequestRole::Admin, None)), ReadScope::Admin);
        assert_eq!(read_scope(&principal(RequestRole::Public, None)), ReadScope::Public);
        assert_eq!(read_scope(&principal(RequestRole::External, None)), ReadScope::Public);
    }

    /// The serde seam the pure test cannot reach: a misspelled `captain_*` field name would
    /// silently deserialize to `None` -> Public everywhere, and the first detector would be a
    /// production smoke timeout. All four keys pinned; a malformed uuid claim fails closed.
    #[test]
    fn app_metadata_claims_deserialize_and_malformed_uuids_fail_closed() {
        let meta: AppMetadata = serde_json::from_str(
            r#"{
                "captain_role": "CUSTOMER",
                "captain_restaurant_id": "00000000-0000-0000-0000-000000000001",
                "captain_restaurant_account_id": "00000000-0000-0000-0000-000000000002",
                "captain_customer_id": "00000000-0000-0000-0000-000000000003",
                "captain_rider_id": "00000000-0000-0000-0000-000000000004"
            }"#,
        )
        .expect("app_metadata blob");
        assert_eq!(claim_uuid(&meta.captain_restaurant_id), Some(uuid::Uuid::from_u128(1)));
        assert_eq!(claim_uuid(&meta.captain_restaurant_account_id), Some(uuid::Uuid::from_u128(2)));
        assert_eq!(claim_uuid(&meta.captain_customer_id), Some(uuid::Uuid::from_u128(3)));
        assert_eq!(claim_uuid(&meta.captain_rider_id), Some(uuid::Uuid::from_u128(4)));

        // Garbage never widens into an identity — indistinguishable from absent, by design.
        assert_eq!(claim_uuid(&Some("not-a-uuid".into())), None);
        assert_eq!(claim_uuid(&None), None);
    }

    // #430's `an_empty_resolver_degrades_to_public` is DELETED deliberately: its premise (a
    // resolver whose DB dependency may be missing) dissolved when resolution became a pure claims
    // function — there is no dependency left to be missing.
}
