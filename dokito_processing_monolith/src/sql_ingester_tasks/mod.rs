use std::convert::identity;

use aide::axum::ApiRouter;
use mycorrhiza_common::tasks::routing::{declare_default_task_route, declare_task_route};
use nypuc_ingest::FixedJurisdictionPurgePrevious;

use crate::sql_ingester_tasks::{
    initialize_config::InitializeConfig, nypuc_ingest::GetMissingDocketsForFixedJurisdiction,
    recreate_dokito_table_schema::RecreateDokitoTableSchema,
};

pub mod database_author_association;
pub mod dokito_sql_connection;
pub mod initialize_config;
pub mod nypuc_ingest;
pub mod recreate_dokito_table_schema;

pub fn add_sql_ingest_task_routes(router: ApiRouter) -> ApiRouter {
    let router = declare_task_route::<FixedJurisdictionPurgePrevious>(router);
    let router = declare_task_route::<GetMissingDocketsForFixedJurisdiction>(router);
    let router = declare_default_task_route::<InitializeConfig>(router);
    let router = declare_default_task_route::<RecreateDokitoTableSchema>(router);

    identity(router)
}
