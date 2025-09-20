use std::{
    env,
    sync::{LazyLock, OnceLock},
};

use sqlx::{PgPool, postgres::PgPoolOptions};

pub static DEFAULT_POSTGRES_CONNECTION_URL: LazyLock<String> = LazyLock::new(|| {
    env::var("POSTGRES_CONNECTION")
        .or(env::var("DATABASE_URL"))
        .expect("POSTGRES_CONNECTION or DATABASE_URL should be set.")
});

static DOKITO_POOL_CELL: OnceLock<PgPool> = OnceLock::new();
pub fn get_dokito_pool() -> Result<&'static PgPool, anyhow::Error> {
    if let Some(inital_pool) = DOKITO_POOL_CELL.get() {
        return Ok(inital_pool);
    }
    let db_url = &**DEFAULT_POSTGRES_CONNECTION_URL;
    let pool = PgPoolOptions::new()
        .max_connections(40)
        .connect_lazy(db_url)?;
    let pool_ref = DOKITO_POOL_CELL.get_or_init(|| pool);
    Ok(pool_ref)
}
