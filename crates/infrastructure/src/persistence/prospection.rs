//! sqlx read-model repository over the materialized `prospectionpipeline` projection table
//! (ADR-0020/0040). Backs the admin `prospectionPipeline` GraphQL query via
//! `application::queries::ProspectionReadRepository`.

use application::queries::{ProspectFilter, ProspectionPipelineRow, ProspectionReadRepository};
use async_trait::async_trait;
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::db_err;
use super::enum_sql::EnumText;
use super::prospection_store;

/// Postgres adapter for the ProspectionPipeline read model.
pub struct PgProspectionRepository {
    pool: PgPool,
}

impl PgProspectionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProspectionReadRepository for PgProspectionRepository {
    /// Scored prospect list, best-score-first (then newest). `min_score` keeps prospects at/above the
    /// threshold; `status` keeps one pipeline stage (bound as its TEXT value, ADR-20260728).
    async fn list(&self, filter: ProspectFilter) -> Result<Vec<ProspectionPipelineRow>, DomainError> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
            "SELECT {} FROM prospectionpipeline WHERE TRUE",
            prospection_store::COLUMNS
        ));
        if let Some(min_score) = filter.min_score {
            qb.push(" AND score >= ").push_bind(min_score);
        }
        if let Some(status) = filter.status {
            qb.push(" AND pipeline_status = ").push_bind(status.to_text());
        }
        qb.push(" ORDER BY score DESC, created_at DESC");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter().map(prospection_store::decode).collect()
    }
}
