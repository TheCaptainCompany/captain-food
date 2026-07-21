//! HubRise **outbound** API client (ADR-20260718-213352) — the "domain enrichment" half.
//!
//! HubRise callbacks for catalog/inventory carry no state, so after a `POST /adapters/hubrise/webhooks` we must
//! PULL the changed resource from HubRise's REST API. This is that client: OAuth2 (authorization-code,
//! non-expiring tokens) + authenticated GETs.
//!
//! Confirmed from HubRise docs:
//! - Token exchange: `POST https://manager.hubrise.com/oauth2/v1/token`, `client_id:client_secret` in the
//!   HTTP Basic `Authorization` header, `code=<auth code>` in an `x-www-form-urlencoded` body. Access
//!   tokens **do not expire** and there is no refresh token. The token response also names the
//!   connection's scope — `account_id`, and `location_id`/`catalog_id` when location-scoped — which is
//!   exactly what the connect flow (issue #20) provisions from.
//! - A connection is authorized against an ACCOUNT (scope `account[...]`) or a single location
//!   (`location[...]`); the token covers everything inside that scope. Tokens are persisted per
//!   connected account in `hubrise_connections` (see `connections.rs`) — the former single global
//!   `HUBRISE_ACCESS_TOKEN` fallback is retired.
//! - API calls: base `https://api.hubrise.com/v1`, access token in the `X-Access-Token` header.
//!
//! NOTE: the resource *paths* below (`/location/{id}/inventory`, `/catalog/{id}`, `/account`,
//! `/locations`, `/catalogs`) are the documented/expected shapes; confirm against the API reference
//! when touching them.

pub const HUBRISE_API_BASE_URL_ENV: &str = "HUBRISE_API_BASE_URL";

const DEFAULT_BASE_URL: &str = "https://api.hubrise.com/v1";
const TOKEN_URL: &str = "https://manager.hubrise.com/oauth2/v1/token";
const ACCESS_TOKEN_HEADER: &str = "X-Access-Token";

/// Why a HubRise API call failed.
#[derive(Debug)]
pub enum HubRiseApiError {
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
            Self::Transport(e) => write!(f, "hubrise transport error: {e}"),
            Self::Status(s) => write!(f, "hubrise API returned status {s}"),
            Self::Decode(e) => write!(f, "hubrise response decode error: {e}"),
        }
    }
}
impl std::error::Error for HubRiseApiError {}

/// The full HubRise token-exchange response: the credential PLUS the connection's scope — the ids the
/// connect flow provisions from. `account_id` is present for account- and location-scoped connections
/// alike; `location_id`/`catalog_id` only when the connection targets one location/catalog.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub location_id: Option<String>,
    #[serde(default)]
    pub catalog_id: Option<String>,
    #[serde(default)]
    pub account_name: Option<String>,
    #[serde(default)]
    pub location_name: Option<String>,
    #[serde(default)]
    pub catalog_name: Option<String>,
}

/// A HubRise API client bound to the API base URL only — the access token is passed PER CALL, because
/// one process serves many connected accounts (tokens live in `hubrise_connections`).
pub struct HubRiseApi {
    base_url: String,
    http: reqwest::Client,
}

/// A stalled partner read must never wedge a worker/endpoint (the SIRENE 6-hour-hang lesson,
/// ADR-20260720-130045): every request carries an explicit timeout.
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("reqwest client")
}

impl HubRiseApi {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), http: http_client() }
    }

    /// Build from env: `HUBRISE_API_BASE_URL` (optional override of the public default).
    pub fn from_env() -> Self {
        Self::new(
            std::env::var(HUBRISE_API_BASE_URL_ENV).unwrap_or_else(|_| DEFAULT_BASE_URL.to_string()),
        )
    }

    /// Authenticated `GET <base><path>` returning parsed JSON. `path` starts with `/`.
    pub async fn get_json(
        &self,
        token: &str,
        path: &str,
    ) -> Result<serde_json::Value, HubRiseApiError> {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let resp = self
            .http
            .get(&url)
            .header(ACCESS_TOKEN_HEADER, token)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| HubRiseApiError::Transport(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(HubRiseApiError::Status(resp.status().as_u16()));
        }
        resp.json().await.map_err(|e| HubRiseApiError::Decode(e.to_string()))
    }

    /// The connected account (name, currency) — scoped by the token.
    pub async fn get_account(&self, token: &str) -> Result<serde_json::Value, HubRiseApiError> {
        self.get_json(token, "/account").await
    }

    /// The locations the token can see (all of the account's for an account-scoped connection).
    pub async fn get_locations(&self, token: &str) -> Result<serde_json::Value, HubRiseApiError> {
        self.get_json(token, "/locations").await
    }

    /// The catalogs the token can see.
    pub async fn get_catalogs(&self, token: &str) -> Result<serde_json::Value, HubRiseApiError> {
        self.get_json(token, "/catalogs").await
    }

    /// Pull a location's inventory (stock levels). Shape is left as JSON until the ACL maps it to
    /// `OfferStockUpdated` (see `enrich.rs`).
    pub async fn get_inventory(
        &self,
        token: &str,
        location_id: &str,
    ) -> Result<serde_json::Value, HubRiseApiError> {
        self.get_json(token, &format!("/location/{location_id}/inventory")).await
    }

    /// Pull a catalog (categories/products/skus/option-lists). Shape is left as JSON until the ACL maps
    /// it to an `ImportCatalog` command (see `enrich.rs`).
    pub async fn get_catalog(
        &self,
        token: &str,
        catalog_id: &str,
    ) -> Result<serde_json::Value, HubRiseApiError> {
        self.get_json(token, &format!("/catalog/{catalog_id}")).await
    }
}

/// Connect: exchange an authorization `code` for the non-expiring access token + connection scope.
/// Driven by `GET /adapters/hubrise/oauth/callback` (the connect flow); the resulting token is stored
/// per account in `hubrise_connections`. `client_id`/`client_secret` are the HubRise app credentials.
pub async fn exchange_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
) -> Result<TokenResponse, HubRiseApiError> {
    exchange_code_at(TOKEN_URL, client_id, client_secret, code).await
}

/// [`exchange_code`] against an explicit token endpoint (tests point this at a local stub).
pub async fn exchange_code_at(
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    code: &str,
) -> Result<TokenResponse, HubRiseApiError> {
    let resp = http_client()
        .post(token_url)
        .basic_auth(client_id, Some(client_secret))
        .form(&[("code", code)])
        .send()
        .await
        .map_err(|e| HubRiseApiError::Transport(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(HubRiseApiError::Status(resp.status().as_u16()));
    }
    resp.json::<TokenResponse>().await.map_err(|e| HubRiseApiError::Decode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_response_parses_the_documented_shape() {
        let body = serde_json::json!({
            "access_token": "b9922a78d3ffab6b95e9d72e88",
            "token_type": "Bearer",
            "account_id": "3r4s3",
            "location_id": "3r4s3-1",
            "catalog_id": "psmlf",
            "customer_list_id": "xab66",
            "account_name": "Bella Pizza",
            "location_name": "Paris",
            "catalog_name": "Bella Pizza"
        });
        let parsed: TokenResponse = serde_json::from_value(body).unwrap();
        assert_eq!(parsed.access_token, "b9922a78d3ffab6b95e9d72e88");
        assert_eq!(parsed.account_id.as_deref(), Some("3r4s3"));
        assert_eq!(parsed.location_id.as_deref(), Some("3r4s3-1"));
        assert_eq!(parsed.catalog_id.as_deref(), Some("psmlf"));
        assert_eq!(parsed.account_name.as_deref(), Some("Bella Pizza"));
    }

    #[test]
    fn token_response_tolerates_a_minimal_body() {
        let parsed: TokenResponse =
            serde_json::from_value(serde_json::json!({ "access_token": "t" })).unwrap();
        assert_eq!(parsed.access_token, "t");
        assert!(parsed.account_id.is_none());
    }

    #[test]
    fn error_messages_are_descriptive() {
        assert!(HubRiseApiError::Status(401).to_string().contains("401"));
    }
}
