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
}

impl Principal {
    fn anonymous() -> Self {
        Self { user_id: None, role: RequestRole::Public }
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
}

struct CachedJwks {
    set: JwkSet,
    fetched: Instant,
}

/// Verifier state: the JWKS endpoint, the expected audience/issuer, an HTTP client, and the cached key set.
pub struct AuthContext {
    jwks_url: Option<String>,
    issuer: Option<String>,
    http: reqwest::Client,
    cache: RwLock<Option<CachedJwks>>,
}

impl AuthContext {
    /// Build from env: `SUPABASE_JWKS_URL` (public keys) and `SUPABASE_URL` (used to derive the expected
    /// `iss = {SUPABASE_URL}/auth/v1`). With no JWKS URL, only `/public` works; other paths return `503`.
    pub fn from_env() -> Arc<Self> {
        let jwks_url = std::env::var("SUPABASE_JWKS_URL").ok().filter(|s| !s.is_empty());
        let issuer = std::env::var("SUPABASE_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|u| format!("{}/auth/v1", u.trim_end_matches('/')));
        if jwks_url.is_none() {
            eprintln!("SUPABASE_JWKS_URL not set — non-public GraphQL paths will return 503 (fail closed)");
        }
        Arc::new(Self { jwks_url, issuer, http: reqwest::Client::new(), cache: RwLock::new(None) })
    }

    /// Authorize a request for `path_role`. `/public` is always allowed (anonymous); every other path
    /// requires a valid bearer token whose `captain_role` equals `path_role`.
    pub async fn authorize(&self, path_role: RequestRole, headers: &HeaderMap) -> Result<Principal, AuthError> {
        if path_role == RequestRole::Public {
            return Ok(Principal::anonymous());
        }
        let token = bearer(headers).ok_or(AuthError::Unauthorized)?;
        let claims = self.verify(token).await?;
        let granted = claims
            .app_metadata
            .captain_role
            .as_deref()
            .map(parse_role)
            .unwrap_or(RequestRole::Customer);
        if role_permitted(path_role, granted) {
            Ok(Principal { user_id: Some(claims.sub), role: path_role })
        } else {
            Err(AuthError::Forbidden)
        }
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
    async fn key_for(&self, kid: &str) -> Result<Jwk, AuthError> {
        if self.stale().await {
            self.refresh().await?;
        }
        if let Some(jwk) = self.lookup(kid).await {
            return Ok(jwk);
        }
        // Unknown kid on a fresh-enough set: force one refetch to absorb a just-rotated key.
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

/// Extract the `Authorization: Bearer <token>` value.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
        .map(str::trim)
        .filter(|t| !t.is_empty())
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
}
