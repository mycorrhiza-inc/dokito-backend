use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use tracing::{info, warn};

use mycorrhiza_common::tasks::ExecuteUserTask;

use crate::{
    jurisdiction_schema_mapping::FixedJurisdiction,
    sql_ingester_tasks::dokito_sql_connection::get_dokito_pool,
};

#[derive(Clone, Copy, Deserialize, JsonSchema)]
pub struct RecreateDokitoTableSchema(pub FixedJurisdiction);

#[async_trait]
impl ExecuteUserTask for RecreateDokitoTableSchema {
    async fn execute_task(self: Box<Self>) -> Result<Value, Value> {
        // You'll need to specify which jurisdiction to recreate schema for
        // This is a placeholder - you may need to modify this based on your use case
        let fixed_jur = self.0; // or get from config/params
        let res = recreate_schema(fixed_jur).await;
        match res {
            Ok(()) => {
                info!("Recreated schema.");
                Ok("Task Completed Successfully".into())
            }
            Err(err) => {
                tracing::error!(error= % err, error_debug= ?err,"Encountered error in recreate_schema");
                Err(err.to_string().into())
            }
        }
    }
    fn get_task_label(&self) -> &'static str {
        "recreate_dokito_table_schema"
    }
    fn get_task_label_static() -> &'static str
    where
        Self: Sized,
    {
        "recreate_dokito_table_schema"
    }
}

pub async fn recreate_schema(fixed_jur: FixedJurisdiction) -> anyhow::Result<()> {
    let pg_schema = fixed_jur.get_postgres_schema_name();
    info!("Got request to recreate schema");
    let pool = get_dokito_pool().await?;
    info!("Created pg pool");

    info!("Dropping existing tables");
    let drop_result = drop_existing_schema(fixed_jur, pool).await;
    if let Err(err) = drop_result {
        warn!(%err,%pg_schema,"Encountered error in dropping schema.  Continuing on and creating new tables.")
    } else {
        info!(%pg_schema,"Tables deleted successfully.")
    }

    info!(%pg_schema,"Creating tables");
    // create_schema(&pool).await?;
    create_schema(fixed_jur, pool).await?;

    info!(%pg_schema,"Successfully recreated schema");

    Ok(())
}

pub async fn drop_existing_schema(
    fixed_jur: FixedJurisdiction,
    pool: &PgPool,
) -> anyhow::Result<()> {
    let pg_schema = fixed_jur.get_postgres_schema_name();
    // migrator.set_ignore_missing(true).undo(pool, 0).await?;

    // Drop schema-specific tables
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {pg_schema} CASCADE"))
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn create_schema(fixed_jur: FixedJurisdiction, pool: &PgPool) -> anyhow::Result<()> {
    let pg_schema = fixed_jur.get_postgres_schema_name();
    // migrator.set_ignore_missing(true).run(pool).await?;

    // Create schema first
    sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {pg_schema}"))
        .execute(pool)
        .await?;

    // Read the migration file content and replace default schema references with dynamic schema
    let migration_sql = include_str!("./migrations/001_dokito_complete.up.sql");
    let schema_specific_sql = migration_sql.replace("public.", &format!("{pg_schema}."));

    info!(%pg_schema, "Executing complete schema creation SQL");

    // Execute the entire SQL as a single raw query
    sqlx::raw_sql(&schema_specific_sql).execute(pool).await?;

    Ok(())
}

pub async fn delete_all_data(fixed_jur: FixedJurisdiction, pool: &PgPool) -> anyhow::Result<()> {
    let pg_schema = fixed_jur.get_postgres_schema_name();
    info!("Starting full data deletion...");

    // Start a transaction

    // Read the truncate file content and replace default schema references with dynamic schema
    let truncate_sql = include_str!("./migrations/truncate_all.sql");
    let schema_specific_truncate_sql = truncate_sql.replace("public.", &format!("{pg_schema}."));

    info!("Executing truncate operations");
    sqlx::raw_sql(&schema_specific_truncate_sql)
        .execute(pool)
        .await?;

    info!("All data deleted successfully ✅");

    Ok(())
}
