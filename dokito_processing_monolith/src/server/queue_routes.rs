use crate::{
    jurisdiction_schema_mapping::FixedJurisdiction,
    server::reprocess_all_handlers::download_dokito_cases_with_dates,
};

use aws_sdk_s3::Client;
use axum::{extract::Path, response::Json};
use chrono::NaiveDate;
use dokito_types::{
    env_vars::DIGITALOCEAN_S3, jurisdictions::JurisdictionInfo, processed::ProcessedGenericDocket,
    raw::RawGenericDocket,
};
use futures::future::join_all;
use non_empty_string::NonEmptyString;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tracing::info;

use crate::{
    processing::{attachments::OpenscrapersExtraData, process_case},
    s3_stuff::{
        DocketAddress, download_openscrapers_object, list_raw_cases_for_jurisdiction, upload_object,
    },
    server::s3_routes::JurisdictionPath,
    sql_ingester_tasks::{
        dokito_sql_connection::get_dokito_pool, nypuc_ingest::ingest_sql_case_with_retries,
    },
};

#[derive(Debug, Clone, Copy)]
pub enum ProcessingAction {
    ProcessOnly,
    IngestOnly,
    ProcessAndIngest,
    UploadRaw,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingActionRawData {
    ProcessOnly,
    ProcessAndIngest,
    UploadRaw,
}

impl From<ProcessingActionRawData> for ProcessingAction {
    fn from(value: ProcessingActionRawData) -> Self {
        match value {
            ProcessingActionRawData::ProcessOnly => Self::ProcessOnly,
            ProcessingActionRawData::ProcessAndIngest => Self::ProcessAndIngest,
            ProcessingActionRawData::UploadRaw => Self::UploadRaw,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingActionIdOnly {
    ProcessOnly,
    IngestOnly,
    ProcessAndIngest,
}

impl From<ProcessingActionIdOnly> for ProcessingAction {
    fn from(value: ProcessingActionIdOnly) -> Self {
        match value {
            ProcessingActionIdOnly::ProcessOnly => Self::ProcessOnly,
            ProcessingActionIdOnly::IngestOnly => Self::IngestOnly,
            ProcessingActionIdOnly::ProcessAndIngest => Self::ProcessAndIngest,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RawDocketsRequest {
    pub action: ProcessingActionRawData,
    pub dockets: Vec<RawGenericDocket>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ByIdsRequest {
    pub action: ProcessingActionIdOnly,
    pub docket_ids: Vec<NonEmptyString>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ByJurisdictionRequest {
    pub action: ProcessingActionIdOnly,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ByDateRangeRequest {
    pub action: ProcessingActionIdOnly,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
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
    RawInfo(Box<RawGenericDocket>),
}

impl From<NonEmptyString> for RawDocketOrGovid {
    fn from(value: NonEmptyString) -> Self {
        RawDocketOrGovid::Govid(value)
    }
}
impl From<RawGenericDocket> for RawDocketOrGovid {
    fn from(value: RawGenericDocket) -> Self {
        RawDocketOrGovid::RawInfo(Box::new(value))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum CaseRawOrProcessed {
    Processed(ProcessedGenericDocket),
    Raw(RawGenericDocket),
}
impl From<ProcessedGenericDocket> for CaseRawOrProcessed {
    fn from(value: ProcessedGenericDocket) -> Self {
        Self::Processed(value)
    }
}

impl From<RawGenericDocket> for CaseRawOrProcessed {
    fn from(value: RawGenericDocket) -> Self {
        Self::Raw(value)
    }
}

#[derive(Debug, Serialize, JsonSchema, Default)]
pub struct ProcessingResponse {
    pub successfully_processed_dockets: Vec<CaseRawOrProcessed>,
    pub success_count: usize,
    pub error_count: usize,
}
async fn execute_processing_single_action(
    info: RawDocketOrGovid,
    action: ProcessingAction,
    fixed_jurisdiction: FixedJurisdiction,
    s3_client: &Client,
    pool: &PgPool,
) -> Result<CaseRawOrProcessed, anyhow::Error> {
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
            let raw_docket =
                download_openscrapers_object::<RawGenericDocket>(s3_client, &docket_addr).await?;
            return Ok(raw_docket.into());
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

    match action {
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
        }
    };

    info!(?gov_id, "Single docket processing completed successfully");
    Ok(processed_docket.into())
}

async fn execute_processing_action(
    gov_ids: Vec<RawDocketOrGovid>,
    action: ProcessingAction,
    jurisdiction: JurisdictionInfo,
) -> Result<ProcessingResponse, String> {
    let s3_client = DIGITALOCEAN_S3.make_s3_client().await;
    let pool = get_dokito_pool().await.map_err(|e| e.to_string())?;
    let fixed_jurisdiction =
        FixedJurisdiction::try_from(&jurisdiction).map_err(|err| err.to_string())?;

    let simultaneous_processers = Semaphore::new(2);
    let completion_futures = gov_ids.into_iter().map(async |info| {
        let _permit = simultaneous_processers.acquire().await;
        execute_processing_single_action(info, action, fixed_jurisdiction, &s3_client, pool).await
    });
    let results = join_all(completion_futures).await;

    let mut response = ProcessingResponse {
        successfully_processed_dockets: vec![],
        success_count: 0,
        error_count: 0,
    };

    for result in results {
        match result {
            Ok(data) => {
                response.success_count += 1;
                response.successfully_processed_dockets.push(data);
            }
            Err(err) => {
                response.error_count += 1;
                info!(?err, "Processing failed for a docket");
            }
        }
    }

    info!(success_count= %response.success_count, error_count=%response.error_count, "Completed processing batch");

    Ok(response)
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
    let response = execute_processing_action(raw_list, request.action.into(), jurisdiction).await?;
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
    let response =
        execute_processing_action(docid_info, request.action.into(), jurisdiction).await?;
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
    let response =
        execute_processing_action(docid_info, request.action.into(), jurisdiction).await?;
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

    let response =
        execute_processing_action(docid_info, request.action.into(), jurisdiction).await?;
    Ok(Json(response))
}
