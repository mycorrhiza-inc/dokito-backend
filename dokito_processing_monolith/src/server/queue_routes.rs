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
use futures::future::join_all;
use sqlx::postgres::PgPoolOptions;
use tokio::sync::Semaphore;
use tracing::info;

use crate::{
    case_worker::ProcessCaseWithoutDownload,
    data_processing_traits::Revalidate,
    processing::process_case,
    server::s3_routes::JurisdictionPath,
    sql_ingester_tasks::{
        dokito_sql_connection::get_dokito_pool, nypuc_ingest::ingest_sql_case_with_retries,
    },
};

use mycorrhiza_common::tasks::{TaskStatusDisplay, workers::add_task_to_queue};

pub async fn manual_fully_process_dockets_right_now(
    Path(JurisdictionPath {
        state,
        jurisdiction_name,
    }): Path<JurisdictionPath>,
    Json(dockets): Json<Vec<RawGenericDocket>>,
) -> Result<Json<Vec<ProcessedGenericDocket>>, String> {
    info!(
        state = %state,
        jurisdiction_name = %jurisdiction_name,
        docket_count = dockets.len(),
        "Starting manual docket processing"
    );

    let jurisdiction = JurisdictionInfo::new_usa(&jurisdiction_name, &state);

    let s3_client = DIGITALOCEAN_S3.make_s3_client().await;

    let extra = (s3_client, jurisdiction);

    // Create a semaphore to limit concurrent processing to 30
    let semaphore = std::sync::Arc::new(Semaphore::new(30));

    // Create futures for concurrent processing with semaphore
    let process_futures = dockets.into_iter().map(|docket| {
        let semaphore_clone = semaphore.clone();
        let extra_clone = extra.clone();

        async move {
            // Acquire permit from semaphore
            let _permit = semaphore_clone.acquire().await.unwrap();
            info!(?docket.case_govid, "Processing docket");
            match process_case(docket, &extra_clone).await {
                Ok(processed) => {
                    info!(?processed.case_govid, "Successfully processed docket");
                    Some(processed)
                }
                Err(err) => {
                    info!(?err, "Failed to process docket");
                    None
                }
            }
        }
    });

    // Execute all processing futures concurrently
    let processed_results = join_all(process_futures).await;

    // Filter out None values (failed processing)
    let processed_dockets: Vec<ProcessedGenericDocket> = processed_results
        .into_iter()
        .flatten() // This removes None values
        .collect();

    let pool = get_dokito_pool().map_err(|_err| "Could not get database connection".to_string())?;
    info!("Database connection established");

    let tries = 3;
    let ignore_existing = false;

    // Create semaphore for SQL ingestion
    let ingest_semaphore = std::sync::Arc::new(Semaphore::new(30));

    // Create futures for concurrent SQL ingestion with semaphore
    let ingest_futures = processed_dockets.iter().map(|processed_docket| {
        let semaphore_clone = ingest_semaphore.clone();
        let pool_clone = pool.clone();
        let mut docket_clone = processed_docket.clone();

        async move {
            // Acquire permit from semaphore
            let _permit = semaphore_clone.acquire().await.unwrap();

            let _ = docket_clone.revalidate().await;
            info!(
                %docket_clone.case_govid,
                %docket_clone.object_uuid,
                tries, ignore_existing, "Ingesting docket into SQL"
            );
            // We don't return anything from this function as errors are handled internally
            let _ = ingest_sql_case_with_retries(
                &mut docket_clone,
                &pool_clone,
                ignore_existing,
                tries,
            )
            .await;
        }
    });

    // Execute all ingestion futures concurrently
    join_all(ingest_futures).await;

    info!(
        processed_count = processed_dockets.len(),
        "Finished manual docket processing"
    );

    Ok(Json(processed_dockets))
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
