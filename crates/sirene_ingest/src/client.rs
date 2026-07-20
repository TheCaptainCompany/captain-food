//! Minimal INSEE Sirene API client (moved here from `infrastructure::integrations::sirene` per
//! ADR-0045, so the CI ingestion binary needs no domain crates): base URL + API key, one `/siret`
//! page per call. Each fetched record keeps BOTH the verbatim JSON (what the staging table stores)
//! and the typed [`Etablissement`] subset (what the ingestion reads its key fields from).

use serde::Deserialize;

use crate::wire::Etablissement;

/// Version-pinned base URL of the Sirene API on the 2024+ INSEE portal.
pub const DEFAULT_BASE_URL: &str = "https://api.insee.fr/api-sirene/3.11";
/// Env var holding the portal API key (repo secret in the GitHub Actions workflow).
pub const INSEE_API_TOKEN_ENV: &str = "INSEE_API_TOKEN";
/// Optional env override of [`DEFAULT_BASE_URL`] (e.g. when INSEE bumps the `/3.11` version segment).
pub const INSEE_API_BASE_URL_ENV: &str = "INSEE_API_BASE_URL";
/// Header carrying the API key on the new INSEE portal (no OAuth2).
pub const API_KEY_HEADER: &str = "X-INSEE-Api-Key-Integration";
/// Sirene's documented maximum `nombre` (page size).
pub const MAX_PAGE_SIZE: u32 = 1000;

/// The crate's error type — a plain message (this crate must not depend on `domain`'s error types).
#[derive(Debug)]
pub struct SireneError(pub String);

impl std::fmt::Display for SireneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SireneError {}

fn err(msg: impl std::fmt::Display) -> SireneError {
    SireneError(msg.to_string())
}

/// One fetched établissement: the verbatim INSEE JSON (stored raw in the staging table) plus the
/// typed subset (SIRET / état / NAF extraction).
#[derive(Debug)]
pub struct SireneRecord {
    /// The record exactly as INSEE returned it — what `external_sirene_restaurants.payload` stores.
    pub raw: serde_json::Value,
    /// The typed deserialization subset of the same record.
    pub etablissement: Etablissement,
}

/// One fetched page, cursor already resolved: `next_cursor = None` ⇔ this was the last page.
#[derive(Debug)]
pub struct SirenePage {
    pub records: Vec<SireneRecord>,
    pub total: u64,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SireneHeader {
    #[serde(default)]
    total: u64,
    #[serde(default)]
    curseur: Option<String>,
    #[serde(default)]
    curseur_suivant: Option<String>,
}

/// Minimal Sirene API client: base URL + API key, one page per call.
pub struct SireneClient {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

/// Per-request ceiling for a single `/siret` call. INSEE pages return in seconds; without this a
/// stalled connection hangs the whole department sweep until GitHub's 6-hour job ceiling force-
/// cancels the run (observed 2026-07-20). Generous enough to never trip on a healthy slow page, but
/// bounded so a dead socket fails the page (then the loop's own retries / department isolation apply)
/// instead of the whole job. Mirrors the explicit timeout on the worker ping in `main.rs`.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// Connection-establishment ceiling — a separate, tighter bound so an unreachable host fails fast.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

impl SireneClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        // A bare `Client::new()` has NO request timeout, so a stalled read blocks forever; always
        // give the client bounded timeouts (fall back to the default client only if the builder,
        // which is infallible in practice, ever errors).
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
        }
    }

    /// Build from env: `INSEE_API_TOKEN` (required) + `INSEE_API_BASE_URL` (optional override).
    pub fn from_env() -> Result<Self, SireneError> {
        let token = std::env::var(INSEE_API_TOKEN_ENV)
            .map_err(|_| err(format!("{INSEE_API_TOKEN_ENV} must be set")))?;
        let base_url =
            std::env::var(INSEE_API_BASE_URL_ENV).unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Ok(Self::new(base_url, token))
    }

    /// Fetch one `/siret` page. `cursor` is `"*"` for the first page, then the previous page's
    /// `next_cursor`. Retries politely on 429 (honouring `Retry-After`, capped) and transient 5xx;
    /// a 404 is Sirene's "zero results" and maps to an empty final page.
    pub async fn fetch_page(
        &self,
        query: &str,
        cursor: &str,
        page_size: u32,
    ) -> Result<SirenePage, SireneError> {
        let url = format!("{}/siret", self.base_url);
        let page_size = page_size.min(MAX_PAGE_SIZE).to_string();
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            let response = self
                .http
                .get(&url)
                .header(API_KEY_HEADER, &self.token)
                .header(reqwest::header::ACCEPT, "application/json")
                .query(&[("q", query), ("nombre", &page_size), ("curseur", cursor)])
                .send()
                .await
                .map_err(|e| err(format!("sirene: request failed: {e}")))?;

            let status = response.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                // Sirene answers 404 for an empty result set — a legitimate "nothing to sync".
                return Ok(SirenePage { records: vec![], total: 0, next_cursor: None });
            }
            if (status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error())
                && attempts < 3
            {
                let wait = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(if status.is_server_error() { 5 } else { 30 })
                    .min(60);
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                continue;
            }
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(err(format!(
                    "sirene: GET /siret returned {status}: {}",
                    body.chars().take(300).collect::<String>()
                )));
            }
            let body = response
                .text()
                .await
                .map_err(|e| err(format!("sirene: reading body: {e}")))?;
            return parse_page(&body, cursor);
        }
    }
}

/// Parse a `/siret` response body into a [`SirenePage`], resolving cursor termination: the run is
/// over when `curseurSuivant` is absent or equal to the cursor we just sent (INSEE's documented
/// end-of-pagination signal), or when the page came back empty. Each record keeps its verbatim JSON.
fn parse_page(body: &str, requested_cursor: &str) -> Result<SirenePage, SireneError> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| err(format!("sirene: unexpected response shape: {e}")))?;
    let header: SireneHeader = value
        .get("header")
        .cloned()
        .ok_or_else(|| err("sirene: response has no header"))
        .and_then(|h| {
            serde_json::from_value(h).map_err(|e| err(format!("sirene: unexpected header shape: {e}")))
        })?;

    let raw_records = value
        .get("etablissements")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut records = Vec::with_capacity(raw_records.len());
    for raw in raw_records {
        let etablissement: Etablissement = serde_json::from_value(raw.clone())
            .map_err(|e| err(format!("sirene: unexpected établissement shape: {e}")))?;
        records.push(SireneRecord { raw, etablissement });
    }

    let sent = header.curseur.as_deref().unwrap_or(requested_cursor);
    let next_cursor = match header.curseur_suivant {
        Some(next) if next != sent && !records.is_empty() => Some(next),
        _ => None,
    };
    Ok(SirenePage { records, total: header.total, next_cursor })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A realistic Sirene 3.11 `/siret` établissement (subset of fields, real shape/casing).
    fn sample_etablissement_json() -> &'static str {
        r#"{
            "siren": "852421099",
            "nic": "00021",
            "siret": "85242109900021",
            "uniteLegale": {
                "denominationUniteLegale": "SARL CHEZ MARCO",
                "activitePrincipaleUniteLegale": "56.10A",
                "etatAdministratifUniteLegale": "A"
            },
            "adresseEtablissement": {
                "numeroVoieEtablissement": "12",
                "typeVoieEtablissement": "RUE",
                "libelleVoieEtablissement": "NATIONALE",
                "codePostalEtablissement": "37000",
                "libelleCommuneEtablissement": "TOURS",
                "codeCommuneEtablissement": "37261"
            },
            "periodesEtablissement": [
                {
                    "dateFin": null,
                    "dateDebut": "2019-07-01",
                    "etatAdministratifEtablissement": "A",
                    "enseigne1Etablissement": "CHEZ MARCO",
                    "denominationUsuelleEtablissement": null,
                    "activitePrincipaleEtablissement": "56.10A"
                }
            ]
        }"#
    }

    #[test]
    fn parses_a_page_keeping_the_raw_payload_and_resolving_cursor_termination() {
        let body = format!(
            r#"{{
                "header": {{ "statut": 200, "message": "OK", "total": 1201, "debut": 0, "nombre": 1,
                             "curseur": "*", "curseurSuivant": "AoEpOTYxODAwNDI1" }},
                "etablissements": [ {} ]
            }}"#,
            sample_etablissement_json()
        );
        let page = parse_page(&body, "*").expect("parse page");
        assert_eq!(page.total, 1201);
        assert_eq!(page.records.len(), 1);
        assert_eq!(page.next_cursor.as_deref(), Some("AoEpOTYxODAwNDI1")); // more pages

        // The typed subset reads the key staging fields…
        let record = &page.records[0];
        assert_eq!(record.etablissement.siret, "85242109900021");
        assert_eq!(record.etablissement.etat(), Some("A"));
        assert_eq!(record.etablissement.naf(), Some("56.10A"));
        // …and the raw payload is the record verbatim (unknown fields like `siren` preserved).
        assert_eq!(record.raw["siren"], serde_json::json!("852421099"));
        assert_eq!(record.raw["siret"], serde_json::json!("85242109900021"));

        // Last page: INSEE echoes the same cursor back as curseurSuivant.
        let last = body.replace("AoEpOTYxODAwNDI1", "*");
        assert!(parse_page(&last, "*").unwrap().next_cursor.is_none());
    }
}
