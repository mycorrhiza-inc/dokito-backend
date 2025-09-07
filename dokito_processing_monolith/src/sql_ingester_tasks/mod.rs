use std::convert::identity;

use aide::axum::ApiRouter;
use mycorrhiza_common::tasks::routing::declare_default_task_route;
use nypuc_ingest::NyPucIngestPurgePrevious;

use crate::sql_ingester_tasks::{
    initialize_config::InitializeConfig, nypuc_ingest::NyPucIngestGetMissingDockets,
    recreate_dokito_table_schema::RecreateDokitoTableSchema,
};

pub mod initialize_config;
pub mod nypuc_ingest;
pub mod recreate_dokito_table_schema;

pub fn add_user_task_routes(router: ApiRouter) -> ApiRouter {
    let router = declare_default_task_route::<NyPucIngestPurgePrevious>(router);
    let router = declare_default_task_route::<NyPucIngestGetMissingDockets>(router);
    let router = declare_default_task_route::<InitializeConfig>(router);
    let router = declare_default_task_route::<RecreateDokitoTableSchema>(router);

    identity(router)
}
