//! Uber Direct adapter config (specs/integrations/uber-direct.md, ADR-20260721-172500). Unlike
//! CoopCycle's per-instance federation registry, Uber Direct is ONE central API — so the config is a
//! single endpoint + credentials, closer to Avelo37. The delta from Avelo37 is authentication: Uber
//! Direct is **OAuth2 client-credentials** (a token manager, `outbound.rs`), not a static bearer key.
//!
//! Gated entirely by `UBER_DIRECT_*` env, exactly as Avelo37's `AVELO37_API_KEY` / CoopCycle's
//! `COOPCYCLE_INSTANCES` gate those adapters: the four REQUIRED vars must all be present, else the
//! composition root keeps the no-op `NoopDeliveryService` stand-in (fail-closed; jobs stay open to
//! the next ranked channel). A PARTIALLY-set config (some vars present, some missing) is a
//! misconfiguration surfaced as `Err` — the operator must see it, not get a silent no-op.

use serde::Deserialize;

/// Required: the Uber Direct customer/organization id (goes in the API path
/// `/v1/customers/{customer_id}/deliveries`).
pub const CUSTOMER_ID_ENV: &str = "UBER_DIRECT_CUSTOMER_ID";
/// Required: OAuth2 client-credentials client id.
pub const CLIENT_ID_ENV: &str = "UBER_DIRECT_CLIENT_ID";
/// Required: OAuth2 client-credentials client secret.
pub const CLIENT_SECRET_ENV: &str = "UBER_DIRECT_CLIENT_SECRET";
/// Required: the webhook signing secret verifying `X-Uber-Signature` (raw-body HMAC-SHA256).
pub const WEBHOOK_SECRET_ENV: &str = "UBER_DIRECT_WEBHOOK_SECRET";
/// Optional: API base URL override (defaults to Uber's production host — overridden in tests).
pub const BASE_URL_ENV: &str = "UBER_DIRECT_BASE_URL";
/// Optional: OAuth2 token endpoint override.
pub const TOKEN_URL_ENV: &str = "UBER_DIRECT_TOKEN_URL";
/// Optional: OAuth2 scope override.
pub const SCOPE_ENV: &str = "UBER_DIRECT_SCOPE";

/// Uber Direct production API host (Create Delivery is POSTed under here).
pub const DEFAULT_BASE_URL: &str = "https://api.uber.com";
/// Uber's OAuth2 token endpoint (client-credentials grant).
pub const DEFAULT_TOKEN_URL: &str = "https://auth.uber.com/oauth/v2/token";
/// Default Uber Direct delivery scope.
pub const DEFAULT_SCOPE: &str = "eats.deliveries";

/// Resolved Uber Direct adapter configuration — the single endpoint + OAuth2 credentials shared by
/// the outbound gateway (create-delivery) and the inbound webhook route (signature secret).
#[derive(Debug, Clone, Deserialize)]
pub struct UberDirectConfig {
    /// Uber Direct customer/organization id — the `{customer_id}` path segment on every API call.
    pub customer_id: String,
    /// OAuth2 client-credentials client id.
    pub client_id: String,
    /// OAuth2 client-credentials client secret.
    pub client_secret: String,
    /// Webhook signing secret (verifies `X-Uber-Signature`).
    pub webhook_secret: String,
    /// API base URL (defaults to [`DEFAULT_BASE_URL`]).
    pub base_url: String,
    /// OAuth2 token endpoint (defaults to [`DEFAULT_TOKEN_URL`]).
    pub token_url: String,
    /// OAuth2 scope (defaults to [`DEFAULT_SCOPE`]).
    pub scope: String,
}

impl UberDirectConfig {
    /// Build from the environment. `Ok(Some)` when all four REQUIRED vars are set; `Ok(None)` when
    /// NONE of them are (the adapter is simply not configured — the caller keeps the no-op stand-in);
    /// `Err` when the config is PARTIAL (some required vars present, others missing) — an operator
    /// misconfiguration that must be seen, not silently downgraded to a no-op.
    pub fn from_env() -> Result<Option<Self>, String> {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let required = [
            (CUSTOMER_ID_ENV, get(CUSTOMER_ID_ENV)),
            (CLIENT_ID_ENV, get(CLIENT_ID_ENV)),
            (CLIENT_SECRET_ENV, get(CLIENT_SECRET_ENV)),
            (WEBHOOK_SECRET_ENV, get(WEBHOOK_SECRET_ENV)),
        ];
        let present: Vec<&str> =
            required.iter().filter(|(_, v)| v.is_some()).map(|(k, _)| *k).collect();
        if present.is_empty() {
            return Ok(None);
        }
        if present.len() != required.len() {
            let missing: Vec<&str> =
                required.iter().filter(|(_, v)| v.is_none()).map(|(k, _)| *k).collect();
            return Err(format!(
                "Uber Direct is partially configured (set: {}; missing: {}) — set all four or none",
                present.join(", "),
                missing.join(", ")
            ));
        }
        Ok(Some(Self {
            customer_id: required[0].1.clone().unwrap(),
            client_id: required[1].1.clone().unwrap(),
            client_secret: required[2].1.clone().unwrap(),
            webhook_secret: required[3].1.clone().unwrap(),
            base_url: get(BASE_URL_ENV).unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            token_url: get(TOKEN_URL_ENV).unwrap_or_else(|| DEFAULT_TOKEN_URL.to_string()),
            scope: get(SCOPE_ENV).unwrap_or_else(|| DEFAULT_SCOPE.to_string()),
        }))
    }

    /// The Create Delivery endpoint for this customer.
    pub fn create_delivery_url(&self) -> String {
        format!(
            "{}/v1/customers/{}/deliveries",
            self.base_url.trim_end_matches('/'),
            self.customer_id
        )
    }
}
