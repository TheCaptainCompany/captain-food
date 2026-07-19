//! HubRise **outbound** API client (ADR-20260718-213352) — the "domain enrichment" half.
//!
//! HubRise callbacks for catalog/inventory carry no state, so after a `POST /adapters/hubrise/webhooks` we must
//! PULL the changed resource from HubRise's REST API. This is that client: OAuth2 (authorization-code,
//! non-expiring tokens) + authenticated GETs.
//!
//! Confirmed from HubRise docs:
//! - Token exchange: `POST https://manager.hubrise.com/oauth2/v1/token`, `client_id:client_secret` in the
//!   HTTP Basic `Authorization` header, `code=<auth code>` in an `x-www-form-urlencoded` body. Access
//!   tokens **do not expire** and there is no refresh token — so for V0 a single connected account's
//!   token is a configured secret (`HUBRISE_ACCESS_TOKEN`). A HubRise Account maps to our RestaurantAccount
//!   (its Locations = our Restaurants), so multi-account support — a connection/token table keyed by
//!   RestaurantAccount — is a later enhancement (needs a connection table → plan mode). See
//!   docs/integrations/hubrise-process.md §0.
//! - API calls: base `https://api.hubrise.com/v1`, access token in the `X-Access-Token` header.
//!
//! NOTE: the resource *paths* below (`/location/{id}/inventory`, `/catalog/{id}`) are the expected shapes;
//! confirm them against the API reference when the domain mapping is wired (see `lib.rs` module docs).

pub const HUBRISE_ACCESS_TOKEN_ENV: &str = "HUBRISE_ACCESS_TOKEN";
pub const HUBRISE_API_BASE_URL_ENV: &str = "HUBRISE_API_BASE_URL";

const DEFAULT_BASE_URL: &str = "https://api.hubrise.com/v1";
const TOKEN_URL: &str = "https://manager.hubrise.com/oauth2/v1/token";
const ACCESS_TOKEN_HEADER: &str = "X-Access-Token";

/// Why a HubRise API call failed.
#[derive(Debug)]
pub enum HubRiseApiError {
    /// `HUBRISE_ACCESS_TOKEN` is unset/empty.
    NotConfigured,
    /// Transport error (DNS/TLS/timeout).
    Transport(String),
    /// Non-2xx HTTP status.
    Status(u16),
    /// Body was not the expected JSON.
    Decode(String),
}

impl std::fmt::Display for HubRiseApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => write!(f, "{HUBRISE_ACCESS_TOKEN_ENV} is not set"),
            Self::Transport(e) => write!(f, "hubrise transport error: {e}"),
            Self::Status(s) => write!(f, "hubrise API returned status {s}"),
            Self::Decode(e) => write!(f, "hubrise response decode error: {e}"),
        }
    }
}
impl std::error::Error for HubRiseApiError {}

/// A HubRise API client bound to one connected location's (non-expiring) access token.
pub struct HubRiseApiClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl HubRiseApiClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), token: token.into(), http: reqwest::Client::new() }
    }

    /// Build from env: `HUBRISE_ACCESS_TOKEN` (required) + `HUBRISE_API_BASE_URL` (optional override).
    pub fn from_env() -> Result<Self, HubRiseApiError> {
        let token = std::env::var(HUBRISE_ACCESS_TOKEN_ENV)
            .ok()
            .filter(|t| !t.trim().is_empty())
            .ok_or(HubRiseApiError::NotConfigured)?;
        let base_url =
            std::env::var(HUBRISE_API_BASE_URL_ENV).unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Ok(Self::new(base_url, token))
    }

    /// Authenticated `GET <base><path>` returning parsed JSON. `path` starts with `/`.
    pub async fn get_json(&self, path: &str) -> Result<serde_json::Value, HubRiseApiError> {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let resp = self
            .http
            .get(&url)
            .header(ACCESS_TOKEN_HEADER, &self.token)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| HubRiseApiError::Transport(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(HubRiseApiError::Status(resp.status().as_u16()));
        }
        resp.json().await.map_err(|e| HubRiseApiError::Decode(e.to_string()))
    }

    /// Pull a location's inventory (stock levels). Shape is left as JSON until the ACL maps it to
    /// `OfferStockUpdated` (deferred — see `lib.rs`).
    pub async fn get_inventory(&self, location_id: &str) -> Result<serde_json::Value, HubRiseApiError> {
        self.get_json(&format!("/location/{location_id}/inventory")).await
    }

    /// Pull a catalog (categories/products/skus/option-lists). Shape is left as JSON until the ACL maps it
    /// to an `ImportCatalog` command (deferred — see `lib.rs`).
    pub async fn get_catalog(&self, catalog_id: &str) -> Result<serde_json::Value, HubRiseApiError> {
        self.get_json(&format!("/catalog/{catalog_id}")).await
    }
}

/// One-time connect: exchange an authorization `code` for a non-expiring access token. Run this once per
/// connected location (e.g. from a small setup tool or an OAuth redirect handler); store the result as
/// `HUBRISE_ACCESS_TOKEN`. `client_id`/`client_secret` are the HubRise app credentials.
pub async fn exchange_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
) -> Result<String, HubRiseApiError> {
    let resp = reqwest::Client::new()
        .post(TOKEN_URL)
        .basic_auth(client_id, Some(client_secret))
        .form(&[("code", code)])
        .send()
        .await
        .map_err(|e| HubRiseApiError::Transport(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(HubRiseApiError::Status(resp.status().as_u16()));
    }
    let body: serde_json::Value =
        resp.json().await.map_err(|e| HubRiseApiError::Decode(e.to_string()))?;
    body.get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| HubRiseApiError::Decode("no access_token in token response".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_requires_a_token() {
        // With the var unset in this test's environment, from_env fails closed.
        std::env::remove_var(HUBRISE_ACCESS_TOKEN_ENV);
        assert!(matches!(HubRiseApiClient::from_env(), Err(HubRiseApiError::NotConfigured)));
    }

    #[test]
    fn error_messages_are_descriptive() {
        assert!(HubRiseApiError::Status(401).to_string().contains("401"));
        assert!(HubRiseApiError::NotConfigured.to_string().contains(HUBRISE_ACCESS_TOKEN_ENV));
    }
}
