use std::convert::identity;

use aide::axum::ApiRouter;
use mycorrhiza_common::tasks::routing::declare_task_route;

use crate::sql_ingester_tasks::recreate_dokito_table_schema::RecreateDokitoTableSchema;

pub mod database_author_association;
pub mod dokito_sql_connection;
pub mod initialize_config;
pub mod nypuc_ingest;
pub mod recreate_dokito_table_schema;

pub fn add_sql_ingest_task_routes(router: ApiRouter) -> ApiRouter {
    let router = declare_task_route::<RecreateDokitoTableSchema>(router);

    identity(router)
}
