//! SIRENE raw ingestion (ADR-0045) — the HTTP-only half of the split sync.
//!
//! This crate owns everything the thin CI job needs and NOTHING more: the INSEE Sirene API client
//! ([`client::SireneClient`]), the wire DTOs ([`wire::Etablissement`] & co, the ONLY place the French
//! INSEE field names exist), the food-service query builder ([`restauration_query`]) and the raw UPSERT
//! into the `external_sirene_restaurants` staging table ([`staging::upsert_staging_row`]). It depends on
//! no domain/application/infrastructure crate, so `cargo build -p sirene_ingest` compiles a small graph
//! and CI executes zero domain logic — the SIRENE Anti-Corruption Layer + the aggregate command handlers
//! run only on the deployed server (`infrastructure::integrations::sync_sirene_worker`), which imports
//! the wire types from HERE.
//!
//! # INSEE Sirene API targeted (researched 2026-07)
//!
//! INSEE decommissioned the legacy `api.insee.fr` OAuth2 (client-credentials) portal in 2024 and moved
//! to the new API portal (`portail-api.insee.fr`). The Sirene REST API we target:
//!
//! - **Base URL**: `https://api.insee.fr/api-sirene/3.11` (version-pinned path; see
//!   [`client::DEFAULT_BASE_URL`], overridable via the `INSEE_API_BASE_URL` env var).
//! - **Auth**: a static API key created on the portal ("integration key"), sent as the HTTP header
//!   `X-INSEE-Api-Key-Integration: <key>` ([`client::API_KEY_HEADER`]), read from `INSEE_API_TOKEN`.
//! - **Endpoint**: `GET /siret` — multi-criteria search over établissements, with `q` (Lucene-ish
//!   query, [`restauration_query`]), `nombre` (page size, max 1000) and `curseur` (deep cursor
//!   pagination: send `*` first, then follow `header.curseurSuivant` until it echoes back).
//! - `404` means ZERO matches (mapped to an empty page); `429` when the ~30 requests/minute quota is
//!   exceeded (retried politely, honouring `Retry-After`).

pub mod client;
pub mod compaction;
pub mod staging;
pub mod sweep;
pub mod wire;

pub use client::{
    SireneClient, SireneError, SirenePage, SireneRecord, API_KEY_HEADER, DEFAULT_BASE_URL,
    INSEE_API_BASE_URL_ENV, INSEE_API_TOKEN_ENV, MAX_PAGE_SIZE,
};
pub use compaction::{
    compact_payloads, journal_message_id, CompactionCounts, COMPACTION_BATCH_SIZE,
};
pub use staging::{upsert_staging_batch, upsert_staging_row, payload_hash, StagingCounts, STAGING_BATCH_SIZE, departments_by_staleness};
pub use wire::{AdresseEtablissement, Etablissement, PeriodeEtablissement, UniteLegale};

/// Code commune INSEE of Tours (the V0 target market).
pub const TOURS_CODE_COMMUNE: &str = "37261";
/// NAF/APE codes for the food-service scope (ADR-0019/0020): 56.10A restauration traditionnelle,
/// 56.10B cafétérias, 56.10C restauration rapide, 56.30Z débits de boissons.
pub const RESTAURATION_NAF_CODES: [&str; 4] = ["56.10A", "56.10B", "56.10C", "56.30Z"];

/// Geographic scope of a sync query. Commune codes embed the department, so department filtering is a
/// prefix wildcard on `codeCommuneEtablissement`.
#[derive(Debug, Clone)]
pub enum SireneScope {
    /// A single code commune INSEE (e.g. [`TOURS_CODE_COMMUNE`]).
    Commune(String),
    /// A whole department (e.g. `"37"` for Indre-et-Loire, `"2A"`, `"971"`).
    Department(String),
}

/// Build the `q=` parameter: currently-active food-service establishments in `scope`.
/// `activitePrincipaleEtablissement` and `etatAdministratifEtablissement` are historized, hence the
/// single `periode(...)` wrapper (both conditions must hold on the SAME — current — period).
pub fn restauration_query(scope: &SireneScope) -> String {
    let naf = RESTAURATION_NAF_CODES
        .iter()
        .map(|code| format!("activitePrincipaleEtablissement:{code}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let geo = match scope {
        SireneScope::Commune(code) => format!("codeCommuneEtablissement:{code}"),
        SireneScope::Department(dept) => format!("codeCommuneEtablissement:{dept}*"),
    };
    format!("{geo} AND periode(etatAdministratifEtablissement:A AND ({naf}))")
}

/// Every French department code the France-wide sweep partitions its queries by: the 96 metropolitan
/// departments (01–19, 2A/2B replacing 20, 21–95) plus the 5 DROM (971–974, 976 — 975/977/978 are
/// collectivités, out of the V0 scope). Partitioning keeps each cursor run comfortably under INSEE's
/// deep-pagination limits and isolates per-department failures.
pub fn french_departments() -> Vec<String> {
    let mut departments: Vec<String> = Vec::with_capacity(101);
    for n in 1..=95 {
        if n == 20 {
            // Corsica split into 2A/2B in 1976; commune codes are prefixed 2A/2B, never 20.
            departments.push("2A".to_string());
            departments.push("2B".to_string());
        } else {
            departments.push(format!("{n:02}"));
        }
    }
    for drom in ["971", "972", "973", "974", "976"] {
        departments.push(drom.to_string());
    }
    departments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_documented_query() {
        let q = restauration_query(&SireneScope::Commune(TOURS_CODE_COMMUNE.into()));
        assert_eq!(
            q,
            "codeCommuneEtablissement:37261 AND periode(etatAdministratifEtablissement:A AND \
             (activitePrincipaleEtablissement:56.10A OR activitePrincipaleEtablissement:56.10B OR \
             activitePrincipaleEtablissement:56.10C OR activitePrincipaleEtablissement:56.30Z))"
        );
        let dept = restauration_query(&SireneScope::Department("37".into()));
        assert!(dept.starts_with("codeCommuneEtablissement:37* AND "));
    }

    #[test]
    fn department_partition_covers_metropolitan_france_plus_drom() {
        let departments = french_departments();
        assert_eq!(departments.len(), 101); // 94 numeric metro + 2A/2B + 5 DROM
        assert!(departments.contains(&"01".to_string()));
        assert!(departments.contains(&"2A".to_string()));
        assert!(departments.contains(&"2B".to_string()));
        assert!(!departments.contains(&"20".to_string())); // Corsica is 2A/2B
        assert!(departments.contains(&"37".to_string())); // Indre-et-Loire (Tours)
        assert!(departments.contains(&"976".to_string())); // Mayotte
        assert!(!departments.contains(&"975".to_string())); // Saint-Pierre-et-Miquelon is a COM
    }
}
