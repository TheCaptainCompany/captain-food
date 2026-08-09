//! Integration test for the prospection read-model slice: a SIRENE-mapped `register_restaurant`
//! (a `domain_events` row) → `ProjectionWorker` (the SAME Restaurant-stream event folds into BOTH the
//! `restaurant` and `prospectionpipeline` rows) → `PgProspectionRepository::list` filters. Needs a real
//! Postgres: set `DATABASE_URL` (see restaurant_write_path.rs for a throwaway docker one-liner). Without
//! it the test SKIPS so `cargo test` stays green offline.

use application::commands::register_restaurant;
use application::ports::Actor;
use application::queries::{ProspectFilter, ProspectionReadRepository};
use domain::generated::scalars::ProspectPipelineStatus;
use infrastructure::integrations::sirene::{
    etablissement_to_command, restaurant_id_for_siret, sirene_system_user_id, Etablissement,
};
use infrastructure::{PgEventStore, PgProspectionRepository, PgRestaurantRepository, ProjectionWorker};

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
async fn registered_prospect_is_folded_and_served_by_the_read_repository() {
    let Some(db) = crate::common::TestDb::acquire("prospection_projection").await else { return };
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

    // 1) SIRENE ACL mapping → one RestaurantRegistered on the Restaurant stream.
    let cmd = etablissement_to_command(&sample_etablissement()).expect("mapping");
    register_restaurant(&store, &restaurants, cmd, &actor).await.expect("register_restaurant");

    // 2) One worker pass folds the event into the prospect row: NEW, unscored, never contacted.
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once");
    let (score, pipeline_status, contacts_count): (i32, String, i32) = sqlx::query_as(
        "SELECT score, pipeline_status, contacts_count FROM prospectionpipeline WHERE restaurant_id = $1",
    )
    .bind(restaurant_id)
    .fetch_one(&pool)
    .await
    .expect("projected prospect row");
    assert_eq!(pipeline_status, "NEW"); // ProspectPipelineStatus::NEW
    assert_eq!(score, 0); // TODO(runtime) weighted score — 0 until the lookup ports land
    assert_eq!(contacts_count, 0);

    // 3) The read repository serves it — and the status filter binds the right TEXT value.
    let repo = PgProspectionRepository::new(pool.clone());
    let all = repo.list(ProspectFilter::default()).await.expect("list all");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].restaurant_id.0, restaurant_id);
    assert_eq!(all[0].pipeline_status, ProspectPipelineStatus::NEW);

    let contacted = repo
        .list(ProspectFilter { status: Some(ProspectPipelineStatus::CONTACTED), ..Default::default() })
        .await
        .expect("list CONTACTED");
    assert!(contacted.is_empty());

    let new = repo
        .list(ProspectFilter { status: Some(ProspectPipelineStatus::NEW), ..Default::default() })
        .await
        .expect("list NEW");
    assert_eq!(new.len(), 1);
}
