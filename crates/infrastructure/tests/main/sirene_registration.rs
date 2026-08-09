//! Integration test for the SIRENE sync slice: a Sirene établissement JSON → ACL mapping →
//! `register_restaurant` + `PgEventStore` (a `domain_events` row) → `ProjectionWorker` (a
//! `restaurant` row) → idempotent re-run. Needs a real Postgres: set `DATABASE_URL` (see
//! restaurant_write_path.rs for a throwaway docker one-liner). Without it the test SKIPS so
//! `cargo test` stays green offline.

use application::commands::register_restaurant;
use application::ports::Actor;
use infrastructure::integrations::sirene::{
    etablissement_to_command, restaurant_id_for_siret, sirene_system_user_id, Etablissement,
};
use infrastructure::{PgEventStore, PgRestaurantRepository, ProjectionWorker};

/// Same realistic Sirene 3.11 shape the ACL unit tests use.
fn sample_etablissement() -> Etablissement {
    serde_json::from_str(
        r#"{
            "siren": "852421099",
            "nic": "00021",
            "siret": "85242109900021",
            "uniteLegale": { "denominationUniteLegale": "SARL CHEZ MARCO",
                             "activitePrincipaleUniteLegale": "56.10A" },
            "adresseEtablissement": {
                "numeroVoieEtablissement": "12",
                "typeVoieEtablissement": "RUE",
                "libelleVoieEtablissement": "NATIONALE",
                "codePostalEtablissement": "37000",
                "libelleCommuneEtablissement": "TOURS",
                "codeCommuneEtablissement": "37261"
            },
            "periodesEtablissement": [ {
                "dateFin": null,
                "etatAdministratifEtablissement": "A",
                "enseigne1Etablissement": "CHEZ MARCO",
                "activitePrincipaleEtablissement": "56.10A"
            } ]
        }"#,
    )
    .expect("parse sample établissement")
}

#[tokio::test]
async fn sirene_mapped_command_flows_through_the_write_path_idempotently() {
    let Some(db) = crate::common::TestDb::acquire("sirene_registration").await else { return };
    let pool = db.pool();

    let store = PgEventStore::new(pool.clone());
    let actor = Actor {
        user_id: sirene_system_user_id(),
        user_type: "EXTERNAL".to_string(),
        domain_id: None,
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    };
    let restaurant_id = restaurant_id_for_siret("85242109900021").0;

    let restaurants = PgRestaurantRepository::new(pool.clone()); // backs the SlugAlreadyTaken check

    // 1) ACL mapping → the ordinary write path appends one RestaurantRegistered.
    let cmd = etablissement_to_command(&sample_etablissement()).expect("mapping");
    register_restaurant(&store, &restaurants, cmd, &actor).await.expect("register_restaurant");

    let (stream, event_type, user_type, payload): (String, String, String, serde_json::Value) =
        sqlx::query_as("SELECT stream_name, event_type, user_type, payload FROM domain_events")
            .fetch_one(&pool)
            .await
            .expect("one event row");
    assert_eq!(stream, format!("Restaurant-{restaurant_id}"));
    assert_eq!(event_type, "RestaurantRegistered");
    assert_eq!(user_type, "EXTERNAL"); // EXTERNAL envelope stamp (ADR-0041)
    assert_eq!(payload["ref"], serde_json::json!("85242109900021"));
    assert_eq!(payload["listingStatus"], serde_json::json!("NON_PARTNER"));

    // 2) The EXISTING projection worker materializes the prospect row.
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once");
    let (slug, display_name, listing_status): (Option<String>, String, String) = sqlx::query_as(
        "SELECT slug, display_name, listing_status FROM restaurant WHERE restaurant_id = $1",
    )
    .bind(restaurant_id)
    .fetch_one(&pool)
    .await
    .expect("projected restaurant row");
    assert_eq!(slug, None, "a registry prospect has no storefront address until one is configured (#220)");
    assert_eq!(display_name, "CHEZ MARCO");
    assert_eq!(listing_status, "NON_PARTNER"); // RestaurantListingStatus::NON_PARTNER

    // 3) Re-run the sync for the same SIRET → deterministic id → absorbed as a no-op.
    let replay = etablissement_to_command(&sample_etablissement()).expect("mapping (replay)");
    register_restaurant(&store, &restaurants, replay, &actor).await.expect("idempotent replay");
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("count events");
    assert_eq!(events, 1);
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM restaurant")
        .fetch_one(&pool)
        .await
        .expect("count restaurant rows");
    assert_eq!(rows, 1);
}
