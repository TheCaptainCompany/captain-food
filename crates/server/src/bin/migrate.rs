//! Captain.Food database migrator (ADR-0043).
//!
//! Run as Render's **Pre-Deploy Command** (`./target/release/migrate`) before the server starts. A thin
//! wrapper over sqlx's own migrator (`server::MIGRATOR`, the same embedded set the server gates on): it
//! applies pending `migrations/NNNN_*.sql` files in ascending order, each in a transaction, recording
//! every applied version in the append-only `_sqlx_migrations` ledger (per-file, checksummed, with a
//! `success` flag; concurrent runs serialize on a Postgres advisory lock). Exits non-zero on any failure
//! so a bad migration blocks the deploy — the new server never gets promoted and the previous one keeps serving.
//!
//! The ledger is APPEND-ONLY. The "current" version is `max(version) WHERE success`; the *required* set is
//! whatever the app embeds. Never delete rows — sqlx treats an absent version as pending (it would re-run
//! it) and validates checksums of applied ones. Contraction (dropping old columns/objects) is itself a new
//! forward migration, not a deletion.

use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("migrate: FAILED — {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is not set")?;
    let pool = PgPoolOptions::new().max_connections(1).connect(&url).await?;

    server::MIGRATOR.run(&pool).await?;

    let max: Option<i64> = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations WHERE success")
        .fetch_one(&pool)
        .await?;
    println!("migrate: OK — schema version = {}", max.unwrap_or(0));
    Ok(())
}
