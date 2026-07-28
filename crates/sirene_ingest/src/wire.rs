//! Wire types — the subset of the Sirene `/siret` response the ingestion and the on-app ACL read.
//! Field names mirror the INSEE JSON exactly (French, camelCase); this crate is the ONLY place they
//! exist — they never leak into `domain` (the ACL in `infrastructure::integrations::sirene` maps them
//! onto ordinary domain commands).

use serde::{Deserialize, Serialize};

/// Trimmed, non-empty, non-"[ND]" (INSEE's redaction marker for non-diffusible data) view of an
/// optional INSEE string field.
pub fn clean(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty() && *v != "[ND]")
}

/// One Sirene établissement (deserialization subset). Additive-tolerant on purpose: every field is
/// optional/defaulted (no `deny_unknown_fields`), so an INSEE shape change never breaks parsing rows
/// already landed in the staging table.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Etablissement {
    pub siret: String,
    #[serde(default)]
    pub unite_legale: Option<UniteLegale>,
    #[serde(default)]
    pub adresse_etablissement: Option<AdresseEtablissement>,
    /// Historized periods, most recent first; the current one has `dateFin: null`.
    #[serde(default)]
    pub periodes_etablissement: Vec<PeriodeEtablissement>,
}

impl Etablissement {
    /// The current (open-ended) period, falling back to the first (most recent) one.
    pub fn current_period(&self) -> Option<&PeriodeEtablissement> {
        self.periodes_etablissement
            .iter()
            .find(|p| p.date_fin.is_none())
            .or_else(|| self.periodes_etablissement.first())
    }

    /// `etatAdministratifEtablissement` of the current period (`A` = active, `F` = fermé/closed).
    pub fn etat(&self) -> Option<&str> {
        self.current_period().and_then(|p| clean(&p.etat_administratif_etablissement))
    }

    /// NAF/APE code of the current period, falling back to the unité légale's.
    pub fn naf(&self) -> Option<&str> {
        self.current_period()
            .and_then(|p| clean(&p.activite_principale_etablissement))
            .or_else(|| {
                self.unite_legale
                    .as_ref()
                    .and_then(|ul| clean(&ul.activite_principale_unite_legale))
            })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UniteLegale {
    #[serde(default)]
    pub denomination_unite_legale: Option<String>,
    #[serde(default)]
    pub denomination_usuelle_1_unite_legale: Option<String>,
    #[serde(default)]
    pub nom_unite_legale: Option<String>,
    #[serde(default)]
    pub nom_usage_unite_legale: Option<String>,
    #[serde(default)]
    pub prenom_1_unite_legale: Option<String>,
    #[serde(default)]
    pub activite_principale_unite_legale: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdresseEtablissement {
    #[serde(default)]
    pub numero_voie_etablissement: Option<String>,
    #[serde(default)]
    pub indice_repetition_etablissement: Option<String>,
    #[serde(default)]
    pub type_voie_etablissement: Option<String>,
    #[serde(default)]
    pub libelle_voie_etablissement: Option<String>,
    #[serde(default)]
    pub complement_adresse_etablissement: Option<String>,
    #[serde(default)]
    pub code_postal_etablissement: Option<String>,
    #[serde(default)]
    pub libelle_commune_etablissement: Option<String>,
    #[serde(default)]
    pub code_commune_etablissement: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodeEtablissement {
    #[serde(default)]
    pub date_fin: Option<String>,
    #[serde(default)]
    pub etat_administratif_etablissement: Option<String>,
    #[serde(default)]
    pub enseigne_1_etablissement: Option<String>,
    #[serde(default)]
    pub denomination_usuelle_etablissement: Option<String>,
    #[serde(default)]
    pub activite_principale_etablissement: Option<String>,
}
