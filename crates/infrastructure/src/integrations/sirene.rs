//! SIRENE Anti-Corruption Layer (ADR-0019/0020/0027/0045) — maps raw INSEE Sirene établissements onto
//! the EXISTING `RegisterRestaurant` command. External facts in → ordinary domain commands out; nothing
//! here touches the read side. A registered prospect then flows through the normal write path:
//! `RestaurantRegistered` in `domain_events` → `ProjectionWorker` folds it into the `restaurant` table →
//! the `restaurants` GraphQL query serves it.
//!
//! Since ADR-0045 the HTTP client, the wire DTOs and the query builder live in the dependency-light
//! `sirene_ingest` crate (so the scheduled CI ingestion builds no domain crates); they are re-exported
//! here for compatibility. This module keeps the pieces that NEED the domain: the deterministic id
//! derivation and the mapping — consumed by the on-app [`super::sync_sirene_worker`], which drains the
//! `external_sirene_restaurants` staging table the ingestion fills.
//!
//! # Mapping decisions (external → `RegisterRestaurant`)
//!
//! - `restaurantId` = **UUIDv5 of the SIRET** under a fixed project namespace ([`restaurant_id_for_siret`]):
//!   stable across syncs, so the client-generated-id idempotency of `register_restaurant` absorbs re-runs.
//! - `ref` = the SIRET (the idempotent external key); `externalIdentifiers` also carry the well-known
//!   `siret` and `naf` keys (see `scalars.yaml#/ExternalIdentifierKey`).
//! - `slug` = slugify(display name) + `-<NIC>` (last 5 SIRET digits) so two establishments with the
//!   same name never collide; matches `^[a-z0-9]+(?:-[a-z0-9]+)*$`.
//! - `displayName` = enseigne → denomination usuelle (période) → denomination usuelle (unité légale) →
//!   denomination (unité légale) → "Prénom Nom" for personnes physiques. INSEE capitalisation is kept as-is.
//! - `listingStatus` = `NON_PARTNER` (a prospect, ADR-0027); `accountId` = None; `openingHours` = []
//!   (SIRENE has none); `location` = None (SIRENE exposes Lambert-93 coordinates, not WGS84 —
//!   conversion is Google-enrichment territory, ADR-0020); `timezone` = Europe/Paris.
//! - `cuisineCategory` best-effort from NAF: 56.10A → TRADITIONAL, 56.10C → FAST_FOOD, otherwise None.
//! - Closed establishments (état `F`/`C` on the current period), SIRETs that are not 14 digits, and
//!   records with no usable name or postal code/city are rejected with a descriptive error — the
//!   worker logs and skips them without aborting the run.

use domain::generated::commands::RegisterRestaurant;
use domain::generated::entities::{Address, ExternalIdentifier};
use domain::generated::scalars::{
    AddressLine, CityName, CountryCode, CuisineCategory, ExternalIdentifierKey, ExternalReference,
    PostalCode, RestaurantDisplayName, RestaurantId, RestaurantListingStatus, Slug, TimeZone,
};
use domain::shared::errors::DomainError;

// The HTTP-only surface moved to `sirene_ingest` (ADR-0045); re-exported so existing paths like
// `infrastructure::integrations::sirene::Etablissement` keep resolving.
pub use sirene_ingest::{
    restauration_query, AdresseEtablissement, Etablissement, PeriodeEtablissement, SireneClient,
    SireneError, SirenePage, SireneRecord, SireneScope, UniteLegale, API_KEY_HEADER,
    DEFAULT_BASE_URL, INSEE_API_BASE_URL_ENV, INSEE_API_TOKEN_ENV, MAX_PAGE_SIZE,
    RESTAURATION_NAF_CODES, TOURS_CODE_COMMUNE,
};

use sirene_ingest::wire::clean;

// ---------------------------------------------------------------------------------------------
// Id derivation
// ---------------------------------------------------------------------------------------------

/// Fixed UUIDv5 namespace for every id this ACL derives. NEVER change it: the derived
/// `restaurantId`s are the idempotency keys of the whole sync.
fn sirene_namespace() -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"https://captain.food/integrations/sirene")
}

/// Deterministic `restaurantId` for a SIRET — the same SIRET always maps to the same aggregate id,
/// so replaying the sync hits `register_restaurant`'s creation-idempotency and is a no-op.
pub fn restaurant_id_for_siret(siret: &str) -> RestaurantId {
    RestaurantId(uuid::Uuid::new_v5(&sirene_namespace(), siret.as_bytes()))
}

/// Fixed system user id stamping the event envelope (`domain_events.user_id`, ADR-0041) for events
/// this synchronizer causes. Deterministic so every run is attributable to the same principal.
pub fn sirene_system_user_id() -> uuid::Uuid {
    uuid::Uuid::new_v5(&sirene_namespace(), b"system:sirene-sync")
}

/// UUIDv5 of an arbitrary seed under the fixed SIRENE namespace — used by the sync worker to derive
/// deterministic `command_journal` ids (`message_id` per send, `cause_id` per staged row) so a
/// re-drained row replays the same journal identity (ADR-20260720-015300, #15).
pub fn sirene_uuid(seed: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&sirene_namespace(), seed.as_bytes())
}

// ---------------------------------------------------------------------------------------------
// Mapping — the actual Anti-Corruption boundary
// ---------------------------------------------------------------------------------------------

/// Lowercase-dash slug matching `^[a-z0-9]+(?:-[a-z0-9]+)*$`, with French accents folded to ASCII.
/// Non-alphanumeric runs collapse to a single dash; leading/trailing dashes are trimmed.
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        let folded: &str = match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'À' | 'Â' | 'Ä' | 'Á' | 'Ã' => "a",
            'ç' | 'Ç' => "c",
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => "e",
            'î' | 'ï' | 'í' | 'Î' | 'Ï' | 'Í' => "i",
            'ô' | 'ö' | 'ó' | 'õ' | 'Ô' | 'Ö' | 'Ó' | 'Õ' => "o",
            'ù' | 'û' | 'ü' | 'ú' | 'Ù' | 'Û' | 'Ü' | 'Ú' => "u",
            'ÿ' | 'Ÿ' | 'ý' => "y",
            'ñ' | 'Ñ' => "n",
            'œ' | 'Œ' => "oe",
            'æ' | 'Æ' => "ae",
            _ => {
                if c.is_ascii_alphanumeric() {
                    out.push(c.to_ascii_lowercase());
                } else if !out.ends_with('-') && !out.is_empty() {
                    out.push('-');
                }
                continue;
            }
        };
        out.push_str(folded);
    }
    out.trim_matches('-').to_string()
}

fn mapping_error(siret: &str, reason: &str) -> DomainError {
    DomainError::Invariant(format!("sirene: établissement {siret}: {reason}"))
}

/// Best-effort display name, in INSEE priority order (shop sign → usual name → legal name → person).
fn display_name(e: &Etablissement) -> Option<String> {
    let period = e.current_period();
    if let Some(p) = period {
        if let Some(name) = clean(&p.enseigne_1_etablissement) {
            return Some(name.to_string());
        }
        if let Some(name) = clean(&p.denomination_usuelle_etablissement) {
            return Some(name.to_string());
        }
    }
    let ul = e.unite_legale.as_ref()?;
    if let Some(name) = clean(&ul.denomination_usuelle_1_unite_legale) {
        return Some(name.to_string());
    }
    if let Some(name) = clean(&ul.denomination_unite_legale) {
        return Some(name.to_string());
    }
    // Personne physique (sole trader): "Prénom Nom(d'usage)".
    let last = clean(&ul.nom_usage_unite_legale).or_else(|| clean(&ul.nom_unite_legale))?;
    match clean(&ul.prenom_1_unite_legale) {
        Some(first) => Some(format!("{first} {last}")),
        None => Some(last.to_string()),
    }
}

/// Best-effort cuisine from NAF: only the two unambiguous codes are mapped; everything else is left
/// for the admin / Google enrichment (ADR-0020) — never guessed.
fn cuisine_from_naf(naf: Option<&str>) -> Option<CuisineCategory> {
    match naf {
        Some("56.10A") => Some(CuisineCategory::TRADITIONAL), // restauration traditionnelle
        Some("56.10C") => Some(CuisineCategory::FAST_FOOD),   // restauration de type rapide
        _ => None,
    }
}

fn address(e: &Etablissement) -> Result<Address, DomainError> {
    let a = e
        .adresse_etablissement
        .as_ref()
        .ok_or_else(|| mapping_error(&e.siret, "no adresseEtablissement"))?;
    let street: String = [
        clean(&a.numero_voie_etablissement),
        clean(&a.indice_repetition_etablissement), // bis/ter…
        clean(&a.type_voie_etablissement),         // RUE/AV/BD… (INSEE abbreviation, kept as-is)
        clean(&a.libelle_voie_etablissement),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    let complement = clean(&a.complement_adresse_etablissement);
    let city = clean(&a.libelle_commune_etablissement)
        .ok_or_else(|| mapping_error(&e.siret, "no commune label"))?;
    // Fall back to the complement, then the commune label, so sparsely-addressed prospects (e.g.
    // food trucks registered at a domicile) are still listed rather than dropped.
    let (line1, line2) = if !street.is_empty() {
        (street, complement.map(str::to_string))
    } else if let Some(c) = complement {
        (c.to_string(), None)
    } else {
        (city.to_string(), None)
    };
    Ok(Address {
        line1: AddressLine(line1),
        line2: line2.map(AddressLine),
        postal_code: PostalCode(
            clean(&a.code_postal_etablissement)
                .ok_or_else(|| mapping_error(&e.siret, "no postal code"))?
                .to_string(),
        ),
        city: CityName(city.to_string()),
        country: CountryCode("FR".to_string()), // SIRENE is the French registry
    })
}

/// Map one Sirene établissement to the existing `RegisterRestaurant` command (pure — no I/O).
/// Rejects closed/unusable records with a descriptive error; the worker logs and skips those.
pub fn etablissement_to_command(e: &Etablissement) -> Result<RegisterRestaurant, DomainError> {
    let siret = e.siret.trim();
    if siret.len() != 14 || !siret.bytes().all(|b| b.is_ascii_digit()) {
        return Err(mapping_error(&e.siret, "SIRET is not 14 digits"));
    }
    if let Some(period) = e.current_period() {
        match period.etat_administratif_etablissement.as_deref() {
            None | Some("A") => {}
            Some(state) => {
                return Err(mapping_error(siret, &format!("not administratively active ({state})")))
            }
        }
    }

    let name =
        display_name(e).ok_or_else(|| mapping_error(siret, "no usable name (enseigne/denomination)"))?;
    let address = address(e)?;
    let naf = e.naf().map(str::to_string);

    let nic = &siret[9..]; // last 5 digits — unique per establishment within the legal unit
    let base = slugify(&name);
    let slug = if base.is_empty() { format!("restaurant-{nic}") } else { format!("{base}-{nic}") };

    let mut external_identifiers = vec![ExternalIdentifier {
        key: ExternalIdentifierKey("siret".to_string()),
        value: siret.to_string(),
    }];
    if let Some(naf) = &naf {
        external_identifiers.push(ExternalIdentifier {
            key: ExternalIdentifierKey("naf".to_string()),
            value: naf.clone(),
        });
    }

    Ok(RegisterRestaurant {
        mode: None,
        restaurant_id: restaurant_id_for_siret(siret),
        account_id: None, // a prospect has no owning RestaurantAccount yet (ADR-0027)
        listing_status: Some(RestaurantListingStatus::NON_PARTNER),
        slug: Slug(slug),
        display_name: RestaurantDisplayName(name),
        contact: None,  // SIRENE exposes no email/phone
        website: None,  // Google-enrichment territory (ADR-0020)
        tags: vec![],
        margin_rate: None,
        cuisine_category: cuisine_from_naf(naf.as_deref()),
        uber_prices_opt_in: None,
        address,
        location: None, // SIRENE coordinates are Lambert-93, not WGS84 — enrichment fills this later
        timezone: Some(TimeZone("Europe/Paris".to_string())), // scope is metropolitan dept 37
        preparation_time_minutes: None,
        opening_hours: vec![], // unknown from SIRENE
        external_identifiers,
        r#ref: Some(ExternalReference(siret.to_string())),
    })
}

// ---------------------------------------------------------------------------------------------
// Tests (pure — no network, no DB). Client/pagination/query tests live in `sirene_ingest`.
// ---------------------------------------------------------------------------------------------

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
                "indiceRepetitionEtablissement": null,
                "typeVoieEtablissement": "RUE",
                "libelleVoieEtablissement": "NATIONALE",
                "complementAdresseEtablissement": null,
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

    fn sample() -> Etablissement {
        serde_json::from_str(sample_etablissement_json()).expect("parse sample établissement")
    }

    #[test]
    fn maps_a_real_shaped_etablissement_to_register_restaurant() {
        let cmd = etablissement_to_command(&sample()).expect("mapping succeeds");

        assert_eq!(cmd.r#ref, Some(ExternalReference("85242109900021".into()))); // ref = SIRET
        assert_eq!(cmd.display_name.0, "CHEZ MARCO"); // enseigne wins over denomination
        assert_eq!(cmd.slug.0, "chez-marco-00021"); // slugified name + NIC suffix
        assert_eq!(cmd.listing_status, Some(RestaurantListingStatus::NON_PARTNER)); // a prospect
        assert_eq!(cmd.account_id, None);
        assert_eq!(cmd.cuisine_category, Some(CuisineCategory::TRADITIONAL)); // NAF 56.10A
        assert!(cmd.opening_hours.is_empty());
        assert_eq!(cmd.address.line1.0, "12 RUE NATIONALE");
        assert_eq!(cmd.address.postal_code.0, "37000");
        assert_eq!(cmd.address.city.0, "TOURS");
        assert_eq!(cmd.address.country.0, "FR");
        assert_eq!(cmd.timezone, Some(TimeZone("Europe/Paris".into())));
        let ids: Vec<(&str, &str)> = cmd
            .external_identifiers
            .iter()
            .map(|i| (i.key.0.as_str(), i.value.as_str()))
            .collect();
        assert_eq!(ids, vec![("siret", "85242109900021"), ("naf", "56.10A")]);
    }

    #[test]
    fn restaurant_id_is_deterministic_from_the_siret() {
        // Stable across calls (and therefore across sync runs → idempotent registration)…
        let a = etablissement_to_command(&sample()).unwrap().restaurant_id;
        let b = etablissement_to_command(&sample()).unwrap().restaurant_id;
        assert_eq!(a, b);
        assert_eq!(a, restaurant_id_for_siret("85242109900021"));
        // …and different for a different SIRET.
        assert_ne!(a, restaurant_id_for_siret("85242109900039"));
        // v5 UUID, version nibble = 5.
        assert_eq!(a.0.get_version_num(), 5);
    }

    #[test]
    fn slug_matches_the_domain_pattern_even_for_accented_messy_names() {
        let mut e = sample();
        e.periodes_etablissement[0].enseigne_1_etablissement =
            Some("  CRÊPERIE L'ÉTOILE — Chez Œdipe & Co !!".into());
        let slug = etablissement_to_command(&e).unwrap().slug.0;
        assert_eq!(slug, "creperie-l-etoile-chez-oedipe-co-00021");
        let re_ok = slug
            .split('-')
            .all(|seg| !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()));
        assert!(re_ok, "slug {slug} must match ^[a-z0-9]+(?:-[a-z0-9]+)*$");
    }

    #[test]
    fn falls_back_to_denomination_and_no_cuisine_for_a_bar() {
        let mut e = sample();
        e.periodes_etablissement[0].enseigne_1_etablissement = None;
        e.periodes_etablissement[0].activite_principale_etablissement = Some("56.30Z".into());
        let cmd = etablissement_to_command(&e).unwrap();
        assert_eq!(cmd.display_name.0, "SARL CHEZ MARCO");
        assert_eq!(cmd.cuisine_category, None); // bars are not guessed
        assert_eq!(
            cmd.external_identifiers.last().map(|i| i.value.clone()),
            Some("56.30Z".into())
        );
    }

    #[test]
    fn rejects_closed_establishments_and_bad_sirets() {
        let mut closed = sample();
        closed.periodes_etablissement[0].etat_administratif_etablissement = Some("F".into());
        assert!(etablissement_to_command(&closed).is_err());

        let mut bad = sample();
        bad.siret = "1234".into();
        assert!(etablissement_to_command(&bad).is_err());

        let mut nameless = sample();
        nameless.periodes_etablissement[0].enseigne_1_etablissement = None;
        nameless.unite_legale = None;
        assert!(etablissement_to_command(&nameless).is_err());
    }
}
