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
use sqlx::PgPool;

/// Fresh copies of the four tables the slice touches (mirrors sirene_registration.rs, plus the
/// `prospectionpipeline` projection table this test asserts on).
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS domain_events, restaurant, prospectionpipeline, projection_checkpoint CASCADE;
        CREATE TABLE domain_events (
          position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
          id UUID NOT NULL UNIQUE,
          stream_name TEXT NOT NULL,
          version INTEGER NOT NULL,
          user_id UUID NOT NULL,
          user_type INTEGER NOT NULL,
          correlation_id UUID NOT NULL,
          cause_id UUID NULL,
          event_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          metadata JSONB NULL,
          occurred_at TIMESTAMPTZ NOT NULL,
          expired_at TIMESTAMPTZ NULL,
          UNIQUE (stream_name, version)
        );
        CREATE TABLE restaurant (
          restaurant_id UUID PRIMARY KEY,
          restaurant_account_id UUID,
          listing_status INTEGER NOT NULL,
          external_identifiers JSONB,
          google_place_id TEXT,
          -- NULLABLE since migrations/20260728020000: a prospect has no slug until one is configured.
          slug TEXT UNIQUE,
          display_name TEXT NOT NULL,
          description TEXT,
          tags JSONB,
          margin_rate TEXT,
          cuisine_category INTEGER,
          uber_prices_opt_in BOOLEAN,
          website TEXT,
          rating TEXT,
          reviews_count INTEGER,
          gbp_order_url TEXT,
          gbp_link_status INTEGER,
          address JSONB NOT NULL,
          location JSONB,
          opening_hours JSONB NOT NULL,
          status INTEGER NOT NULL,
          order_acceptance INTEGER NOT NULL,
          default_currency TEXT NOT NULL,
          timezone TEXT,
          preparation_time_minutes INTEGER,
          created_at TIMESTAMPTZ NOT NULL,
          updated_at TIMESTAMPTZ NOT NULL
        );
        CREATE TABLE prospectionpipeline (
          restaurant_id UUID PRIMARY KEY,
          score INTEGER NOT NULL,
          pipeline_status INTEGER NOT NULL,
          contacts_count INTEGER NOT NULL,
          last_contacted_at TIMESTAMPTZ,
          replied_at TIMESTAMPTZ,
          created_at TIMESTAMPTZ NOT NULL,
          updated_at TIMESTAMPTZ NOT NULL
        );
        CREATE TABLE projection_checkpoint (
          projector  TEXT        PRIMARY KEY,
          position   BIGINT      NOT NULL DEFAULT 0,
          updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        "#,
    )
    .execute(pool)
    .await
    .expect("reset schema");
}

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
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP registered_prospect_is_folded_and_served_by_the_read_repository: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    let store = PgEventStore::new(pool.clone());
    let actor = Actor {
        user_id: sirene_system_user_id(),
        user_type: 6, // UserType::EXTERNAL ordinal
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
    let (score, pipeline_status, contacts_count): (i32, i32, i32) = sqlx::query_as(
        "SELECT score, pipeline_status, contacts_count FROM prospectionpipeline WHERE restaurant_id = $1",
    )
    .bind(restaurant_id)
    .fetch_one(&pool)
    .await
    .expect("projected prospect row");
    assert_eq!(pipeline_status, 0); // ProspectPipelineStatus::NEW ordinal
    assert_eq!(score, 0); // TODO(runtime) weighted score — 0 until the lookup ports land
    assert_eq!(contacts_count, 0);

    // 3) The read repository serves it — and the status filter binds the right ordinal.
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
