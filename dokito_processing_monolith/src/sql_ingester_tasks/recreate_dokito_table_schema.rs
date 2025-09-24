use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use sqlx::{Executor, PgPool, migrate::Migrator};
use tracing::info;

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
    info!("Got request to recreate schema");
    let pool = get_dokito_pool()?;
    info!("Created pg pool");

    let mut migrator = sqlx::migrate!("./src/sql_ingester_tasks/migrations");

    let num_migrations = migrator.iter().count();
    info!(%num_migrations,"Created sqlx migrator");

    info!("Dropping existing tables");
    drop_existing_schema(fixed_jur, &pool, &mut migrator).await?;

    info!("Creating tables");
    // create_schema(&pool).await?;
    create_schema(fixed_jur, &pool, &mut migrator).await?;

    info!("Successfully recreated schema");

    Ok(())
}

pub async fn drop_existing_schema(
    fixed_jur: FixedJurisdiction,
    pool: &PgPool,
    _migrator: &mut Migrator,
) -> anyhow::Result<()> {
    let pg_schema = fixed_jur.get_postgres_schema_name();
    // migrator.set_ignore_missing(true).undo(pool, 0).await?;

    // Drop schema-specific tables
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {pg_schema} CASCADE"))
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn create_schema(
    fixed_jur: FixedJurisdiction,
    pool: &PgPool,
    _migrator: &mut Migrator,
) -> anyhow::Result<()> {
    let pg_schema = fixed_jur.get_postgres_schema_name();
    // migrator.set_ignore_missing(true).run(pool).await?;

    // Create schema first
    sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {pg_schema}"))
        .execute(pool)
        .await?;

    // Read the migration file content and replace default schema references with dynamic schema
    let migration_sql = include_str!("./migrations/001_dokito_complete.up.sql");
    let schema_specific_sql = migration_sql.replace("public.", &format!("{pg_schema}."));

    sqlx::query(&schema_specific_sql).execute(pool).await?;
    Ok(())
}

pub async fn delete_all_data(fixed_jur: FixedJurisdiction, pool: &PgPool) -> anyhow::Result<()> {
    let pg_schema = fixed_jur.get_postgres_schema_name();
    info!("Starting full data deletion...");

    // Start a transaction
    let mut tx = pool.begin().await?;

    // Disable statement timeout just for this transaction
    sqlx::query("SET LOCAL statement_timeout = 0;")
        .execute(&mut *tx)
        .await?;
    info!("Disabled statement_timeout for this transaction");

    // Drop relation tables first (with CASCADE)
    info!("Deleting from fillings_filed_by_org_relation");
    sqlx::query(&format!(
        "TRUNCATE {pg_schema}.fillings_filed_by_org_relation CASCADE"
    ))
    .execute(&mut *tx)
    .await?;

    info!("Deleting from fillings_on_behalf_of_org_relation");
    sqlx::query(&format!(
        "TRUNCATE {pg_schema}.fillings_on_behalf_of_org_relation CASCADE"
    ))
    .execute(&mut *tx)
    .await?;

    // Attachments
    info!("Deleting from attachments");
    sqlx::query(&format!("TRUNCATE {pg_schema}.attachments CASCADE"))
        .execute(&mut *tx)
        .await?;

    // Organizations
    info!("Deleting from organizations");
    sqlx::query(&format!("TRUNCATE {pg_schema}.organizations CASCADE"))
        .execute(&mut *tx)
        .await?;

    // Fillings
    info!("Deleting from fillings");
    sqlx::query(&format!("TRUNCATE {pg_schema}.fillings CASCADE"))
        .execute(&mut *tx)
        .await?;

    // Dockets
    info!("Deleting from dockets");
    sqlx::query(&format!("TRUNCATE {pg_schema}.dockets CASCADE"))
        .execute(&mut *tx)
        .await?;

    // Commit once everything is successful
    tx.commit().await?;
    info!("All data deleted successfully ✅");

    Ok(())
}
