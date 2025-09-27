use crate::{
    jurisdiction_schema_mapping::FixedJurisdiction,
    server::reprocess_all_handlers::download_dokito_cases_with_dates,
};

use aide::{self, axum::IntoApiResponse, transform::TransformOperation};
use aws_sdk_s3::Client;
use axum::{
    extract::Path,
    response::{IntoResponse, Json},
};
use chrono::NaiveDate;
use dokito_types::{
    env_vars::DIGITALOCEAN_S3,
    jurisdictions::JurisdictionInfo,
    processed::ProcessedGenericDocket,
    raw::{RawDocketWithJurisdiction, RawGenericDocket},
};
use futures::future::join_all;
use non_empty_string::NonEmptyString;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tracing::info;

use crate::{
    case_worker::ProcessCaseWithoutDownload,
    data_processing_traits::Revalidate,
    processing::{attachments::OpenscrapersExtraData, process_case},
    s3_stuff::{
        DocketAddress, download_openscrapers_object, list_raw_cases_for_jurisdiction, upload_object,
    },
    server::s3_routes::JurisdictionPath,
    sql_ingester_tasks::{
        dokito_sql_connection::get_dokito_pool, nypuc_ingest::ingest_sql_case_with_retries,
    },
};

use mycorrhiza_common::tasks::{TaskStatusDisplay, workers::add_task_to_queue};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingAction {
    ProcessOnly,
    IngestOnly,
    ProcessAndIngest,
    UploadRaw,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RawDocketsRequest {
    pub action: ProcessingAction,
    pub dockets: Vec<RawGenericDocket>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ByIdsRequest {
    pub action: ProcessingAction,
    pub docket_ids: Vec<NonEmptyString>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ByJurisdictionRequest {
    pub action: ProcessingAction,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ByDateRangeRequest {
    pub action: ProcessingAction,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct ProcessingResponse {
    pub processed_dockets: Vec<ProcessedGenericDocket>,
    pub success_count: usize,
    pub error_count: usize,
}

// create a standard interface for handling all the possible ingest forms for the dockets. There
// should be three ways to take in dockets.
// 1) a vec of RawGenericDocket.
// 2) a list of NonEmpty docket_govid strings.
// 3) take a jurisdiction and get all dockets that are missing in the postgres database from that
//    jurisdiction.
// 4) Give a daterange and ingest all dockets inside that daterange, when fetched from the
//    database.
// Then from here do one of three things with it.
// 1) Just process the completed dockets.
// 2) Just ingest the already existing completed dockets to postgres.
// 3) Do both, force reprocessing of the existing dockets then ingest the results to postgres.
// 4) Just upload the RawGenericDockets to s3 and do nothing else. (On ingest options 2,3,4 this
//    should be disabled or do nothing.)
//
//
// 5) Go ahead and create a single unified function that processes everything, but then offer
//    different routes that support these modalities.

fn filter_out_empty_strings(strings: Vec<String>) -> Vec<NonEmptyString> {
    strings
        .into_iter()
        .filter_map(|s| NonEmptyString::try_from(s).ok())
        .collect()
}

#[derive(Clone)]
enum RawDocketOrGovid {
    Govid(NonEmptyString),
    RawInfo(RawGenericDocket),
}

impl From<NonEmptyString> for RawDocketOrGovid {
    fn from(value: NonEmptyString) -> Self {
        RawDocketOrGovid::Govid(value)
    }
}
impl From<RawGenericDocket> for RawDocketOrGovid {
    fn from(value: RawGenericDocket) -> Self {
        RawDocketOrGovid::RawInfo(value)
    }
}

async fn execute_processing_single_action(
    info: RawDocketOrGovid,
    action: ProcessingAction,
    fixed_jurisdiction: FixedJurisdiction,
    s3_client: &Client,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    let gov_id = match &info {
        RawDocketOrGovid::Govid(govid) => govid.clone(),
        RawDocketOrGovid::RawInfo(raw) => raw.case_govid.clone(),
    };
    let jur_info = JurisdictionInfo::from(fixed_jurisdiction);

    info!(
        ?gov_id,
        ?action,
        jurisdiction = %jur_info.jurisdiction,
        state = %jur_info.state,
        "Starting single docket processing"
    );

    let docket_addr = DocketAddress {
        docket_govid: gov_id.to_string(),
        jurisdiction: jur_info.clone(),
    };

    if let RawDocketOrGovid::RawInfo(raw) = info {
        info!(?gov_id, "Uploading raw docket to S3");
        upload_object::<RawGenericDocket>(s3_client, &docket_addr, &raw).await?;
        info!(?gov_id, "Successfully uploaded raw docket to S3");
    }

    let mut processed_docket = match action {
        ProcessingAction::ProcessOnly | ProcessingAction::ProcessAndIngest => {
            info!(?gov_id, "Downloading raw docket from S3");
            let raw_docket =
                download_openscrapers_object::<RawGenericDocket>(s3_client, &docket_addr).await?;
            info!(?gov_id, "Successfully downloaded raw docket from S3");

            info!(?gov_id, "Starting docket processing");
            let extra_data = OpenscrapersExtraData {
                jurisdiction_info: jur_info.clone(),
                fixed_jurisdiction,
                s3_client: DIGITALOCEAN_S3.make_s3_client().await,
            };
            // Handles both fetching the cached s3 processed docket and uploading the result.
            let processed_docket = process_case(raw_docket, extra_data).await?;
            info!(?gov_id, "Successfully processed docket");
            processed_docket
        }
        ProcessingAction::UploadRaw => {
            info!(
                ?gov_id,
                "Upload-only action completed, no further processing needed"
            );
            return Ok(());
        }
        ProcessingAction::IngestOnly => {
            info!(
                ?gov_id,
                "Downloading processed docket from S3 for ingestion"
            );
            let proccessed_docket =
                download_openscrapers_object::<ProcessedGenericDocket>(s3_client, &docket_addr)
                    .await?;
            info!(?gov_id, "Successfully downloaded processed docket from S3");
            proccessed_docket
        }
    };

    let final_upload = match action {
        ProcessingAction::IngestOnly | ProcessingAction::ProcessAndIngest => {
            info!(?gov_id, "Starting SQL ingestion");
            const TRIES: usize = 3;
            ingest_sql_case_with_retries(
                &mut processed_docket,
                fixed_jurisdiction,
                pool,
                false,
                TRIES,
            )
            .await?;
            info!(?gov_id, "Successfully completed SQL ingestion");
        }
        _ => {
            info!(?gov_id, "No ingestion required for this action, completing");
            return Ok(());
        }
    };

    info!(?gov_id, "Single docket processing completed successfully");
    Ok(final_upload)
}

async fn execute_processing_action(
    gov_ids: Vec<RawDocketOrGovid>,
    action: ProcessingAction,
    jurisdiction: JurisdictionInfo,
) -> Result<ProcessingResponse, String> {
    let s3_client = DIGITALOCEAN_S3.make_s3_client().await;
    let pool = get_dokito_pool().map_err(|e| e.to_string())?;
    let fixed_jurisdiction =
        FixedJurisdiction::try_from(&jurisdiction).map_err(|err| err.to_string())?;

    let simultaneous_processers = Semaphore::new(20);
    let completion_futures = gov_ids.into_iter().map(async |info| {
        let _permit = simultaneous_processers.acquire().await;
        execute_processing_single_action(info, action, fixed_jurisdiction, &s3_client, pool).await
    });
    let results = join_all(completion_futures).await;

    let mut success_count = 0;
    let mut error_count = 0;

    for result in results {
        match result {
            Ok(_) => success_count += 1,
            Err(err) => {
                error_count += 1;
                info!(?err, "Processing failed for a docket");
            }
        }
    }

    info!(success_count, error_count, "Completed processing batch");

    Ok(ProcessingResponse {
        processed_dockets: vec![], // Not returning individual dockets for performance
        success_count,
        error_count,
    })
}

pub async fn raw_dockets_endpoint(
    Path(JurisdictionPath {
        state,
        jurisdiction_name,
    }): Path<JurisdictionPath>,
    Json(request): Json<RawDocketsRequest>,
) -> Result<Json<ProcessingResponse>, String> {
    info!(
        state = %state,
        jurisdiction_name = %jurisdiction_name,
        action = ?request.action,
        docket_count = request.dockets.len(),
        "Processing raw dockets request"
    );

    let jurisdiction = JurisdictionInfo::new_usa(&jurisdiction_name, &state);

    let raw_list = request
        .dockets
        .into_iter()
        .map(RawDocketOrGovid::from)
        .collect();
    let response = execute_processing_action(raw_list, request.action, jurisdiction).await?;
    Ok(Json(response))
}

pub async fn by_ids_endpoint(
    Path(JurisdictionPath {
        state,
        jurisdiction_name,
    }): Path<JurisdictionPath>,
    Json(request): Json<ByIdsRequest>,
) -> Result<Json<ProcessingResponse>, String> {
    info!(
        state = %state,
        jurisdiction_name = %jurisdiction_name,
        action = ?request.action,
        id_count = request.docket_ids.len(),
        "Processing by-ids request"
    );

    let jurisdiction = JurisdictionInfo::new_usa(&jurisdiction_name, &state);
    let docid_info = request
        .docket_ids
        .into_iter()
        .map(RawDocketOrGovid::from)
        .collect();
    let response = execute_processing_action(docid_info, request.action, jurisdiction).await?;
    Ok(Json(response))
}

pub async fn by_jurisdiction_endpoint(
    Path(JurisdictionPath {
        state,
        jurisdiction_name,
    }): Path<JurisdictionPath>,
    Json(request): Json<ByJurisdictionRequest>,
) -> Result<Json<ProcessingResponse>, String> {
    info!(
        state = %state,
        jurisdiction_name = %jurisdiction_name,
        action = ?request.action,
        "Processing by-jurisdiction request"
    );

    let jurisdiction = JurisdictionInfo::new_usa(&jurisdiction_name, &state);
    let s3_client = DIGITALOCEAN_S3.make_s3_client().await;

    let gov_ids = list_raw_cases_for_jurisdiction(&s3_client, &jurisdiction)
        .await
        .map_err(|e| e.to_string())?;

    info!(
        found_docket_count = gov_ids.len(),
        "Found dockets for jurisdiction"
    );
    let nonempty_gov_ids = filter_out_empty_strings(gov_ids);

    let docid_info = nonempty_gov_ids
        .into_iter()
        .map(RawDocketOrGovid::from)
        .collect();
    let response = execute_processing_action(docid_info, request.action, jurisdiction).await?;
    Ok(Json(response))
}

pub async fn by_daterange_endpoint(
    Path(JurisdictionPath {
        state,
        jurisdiction_name,
    }): Path<JurisdictionPath>,
    Json(request): Json<ByDateRangeRequest>,
) -> Result<Json<ProcessingResponse>, String> {
    info!(
        state = %state,
        jurisdiction_name = %jurisdiction_name,
        action = ?request.action,
        start_date = %request.start_date,
        end_date = %request.end_date,
        "Processing by-daterange request"
    );

    let jurisdiction = JurisdictionInfo::new_usa(&jurisdiction_name, &state);
    let fixed_jur = FixedJurisdiction::try_from(&jurisdiction).map_err(|e| e.to_string())?;
    let caselist_by_dates = download_dokito_cases_with_dates(fixed_jur)
        .await
        .map_err(|e| e.to_string())?;

    // Filter cases by date range
    let filtered_docket_ids: Vec<String> = caselist_by_dates
        .range(request.start_date..=request.end_date)
        .map(|(_, docket_id)| docket_id.clone())
        .collect();

    info!(
        filtered_count = filtered_docket_ids.len(),
        start_date = %request.start_date,
        end_date = %request.end_date,
        "Filtered dockets by date range"
    );

    let nonempty_gov_ids = filter_out_empty_strings(filtered_docket_ids);
    let docid_info = nonempty_gov_ids
        .into_iter()
        .map(RawDocketOrGovid::from)
        .collect();

    let response = execute_processing_action(docid_info, request.action, jurisdiction).await?;
    Ok(Json(response))
}

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

    let fixed_jurisdiction =
        FixedJurisdiction::try_from(&jurisdiction).map_err(|err| err.to_string())?;

    let extra = OpenscrapersExtraData {
        s3_client,
        jurisdiction_info: jurisdiction,
        fixed_jurisdiction,
    };

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
            match process_case(docket, extra_clone).await {
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
                fixed_jurisdiction,
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
