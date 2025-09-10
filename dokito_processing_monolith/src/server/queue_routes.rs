use aide::{self, axum::IntoApiResponse, transform::TransformOperation};
use axum::{
    extract::Path,
    response::{IntoResponse, Json},
};
use dokito_types::{
    env_vars::DIGITALOCEAN_S3,
    jurisdictions::JurisdictionInfo,
    processed::ProcessedGenericDocket,
    raw::{RawDocketWithJurisdiction, RawGenericDocket},
};
use sqlx::postgres::PgPoolOptions;
use tracing::info;

use crate::{
    case_worker::ProcessCaseWithoutDownload,
    processing::process_case,
    server::s3_routes::JurisdictionPath,
    sql_ingester_tasks::nypuc_ingest::{
        DEFAULT_POSTGRES_CONNECTION_URL, ingest_sql_case_with_retries, ingest_sql_nypuc_case,
    },
};

use mycorrhiza_common::tasks::{TaskStatusDisplay, workers::add_task_to_queue};

pub async fn manual_fully_process_dockets_right_now(
    Json(dockets): Json<Vec<RawGenericDocket>>,
    Path(JurisdictionPath {
        state,
        jurisdiction_name,
    }): Path<JurisdictionPath>,
) -> Result<Json<Vec<ProcessedGenericDocket>>, String> {
    let jurisdiction = JurisdictionInfo::new_usa(&jurisdiction_name, &state);
    let s3_client = DIGITALOCEAN_S3.make_s3_client().await;
    let extra = (s3_client, jurisdiction);
    let mut return_list = vec![];
    for docket in dockets {
        let res = process_case(docket, &extra).await;
        if let Ok(processed) = res {
            return_list.push(processed);
        }
    }
    let db_url = &**DEFAULT_POSTGRES_CONNECTION_URL;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(db_url)
        .await
        .unwrap();
    let tries = 3;
    let ignore_existing = false;
    for processed_docket in return_list.iter() {
        let _res =
            ingest_sql_case_with_retries(processed_docket, &pool, ignore_existing, tries).await;
    }
    Ok(Json(return_list))
}

pub async fn submit_case_to_queue_without_download(
    Json(case): Json<RawDocketWithJurisdiction>,
) -> impl IntoApiResponse {
    let priority = 0;
    info!(case_number = %case.docket.case_govid, %priority, "Request received to submit case to queue");
    let res = add_task_to_queue(ProcessCaseWithoutDownload(case), priority).await;
    (
        axum::http::StatusCode::OK,
        Json(TaskStatusDisplay::from(res)),
    )
        .into_response()
}

pub fn submit_case_to_queue_docs(op: TransformOperation) -> TransformOperation {
    op.description("Submit a case to the processing queue.")
        .response::<200, Json<TaskStatusDisplay>>()
}
