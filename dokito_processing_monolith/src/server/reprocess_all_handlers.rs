use std::collections::{BTreeMap, HashSet};

use aws_sdk_s3::Client;
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use futures_util::{StreamExt, stream};
use mycorrhiza_common::tasks::ExecuteUserTask;
use non_empty_string::NonEmptyString;
use rand::{SeedableRng, rngs::SmallRng, seq::SliceRandom};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use tracing::info;

use crate::{
    data_processing_traits::DownloadIncomplete,
    processing::ReprocessDocketInfo,
    s3_stuff::{
        DocketAddress, download_openscrapers_object, list_processed_cases_for_jurisdiction,
        list_raw_cases_for_jurisdiction, make_s3_client, upload_object,
    },
    sql_ingester_tasks::nypuc_ingest::DEFAULT_POSTGRES_CONNECTION_URL,
    types::{jurisdictions::JurisdictionInfo, processed::ProcessedGenericDocket},
};

const fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ReprocessJurisdictionInfo {
    pub jurisdiction: JurisdictionInfo,
    pub ignore_cached_older_than: Option<DateTime<Utc>>,
    #[serde(default = "default_true")]
    pub only_process_missing: bool,
}
pub async fn reprocess_dockets(
    Json(payload): Json<ReprocessJurisdictionInfo>,
) -> Result<String, String> {
    let s3_client = make_s3_client().await;

    let mut initial_caselist_to_process = get_initial_govid_list_to_process(
        &s3_client,
        &payload.jurisdiction,
        payload.only_process_missing,
    )
    .await
    .map_err(|e| e.to_string())?;

    // Randomizing the list just to insure that the processing difficulty is uniform.
    let mut rng = SmallRng::from_os_rng();
    initial_caselist_to_process.shuffle(&mut rng);
    let boxed_tasks = initial_caselist_to_process.into_iter().map(|docket_govid| {
        let task_info = ReprocessDocketInfo {
            docket_govid,
            jurisdiction: payload.jurisdiction.clone(),
            only_process_missing: payload.only_process_missing,
            ignore_cachced_if_older_than: payload.ignore_cached_older_than,
        };
        Box::new(task_info)
    });
    let _results = stream::iter(boxed_tasks)
        .map(ExecuteUserTask::execute_task)
        .buffer_unordered(30)
        .collect::<Vec<_>>()
        .await;

    Ok("Successfully added processing tasks to queue".to_string())
}

async fn get_initial_govid_list_to_process(
    s3_client: &Client,
    jur_info: &JurisdictionInfo,
    only_process_missing: bool,
) -> anyhow::Result<Vec<String>> {
    let raw_caselist = list_raw_cases_for_jurisdiction(s3_client, jur_info).await?;
    if !only_process_missing {
        return Ok(raw_caselist);
    }
    let processed_govid_list = list_processed_cases_for_jurisdiction(s3_client, jur_info).await?;
    let mut raw_govid_map = raw_caselist.into_iter().collect::<HashSet<_>>();
    for processed_govid in processed_govid_list.iter() {
        raw_govid_map.remove(processed_govid);
    }
    Ok(raw_govid_map.into_iter().collect())
}

pub async fn download_dokito_cases_with_dates() -> anyhow::Result<BTreeMap<NaiveDate, String>> {
    let db_url = &**DEFAULT_POSTGRES_CONNECTION_URL;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(db_url)
        .await?;
    let results = sqlx::query!("SELECT docket_govid, opened_date FROM public.dockets")
        .fetch_all(&pool)
        .await?;
    let bmap = results
        .into_iter()
        .map(|val| (val.opened_date, val.docket_govid))
        .collect();

    Ok(bmap)
}

pub async fn handle_download_all_missing_hashes_newest(
    Json(payload): Json<JurisdictionInfo>,
) -> Result<String, String> {
    info!("Downloading all hashes starting from newest.");
    let s3_client = make_s3_client().await;
    let cases_with_dates = download_dokito_cases_with_dates()
        .await
        .map_err(|e| e.to_string())?;
    let caselist: Vec<String> = cases_with_dates
        .into_iter()
        .rev() // reverse iteration (newest → oldest)
        .map(|(_, docketid)| docketid)
        .collect();
    info!(length = %caselist.len(),"Successfully got caselist, beginning to download.");

    let _res = download_attachments_from_docids(caselist, &s3_client, &payload).await;
    Ok("Completed Successfully".into())
}
pub async fn handle_download_all_missing_hashes_random(
    Json(payload): Json<JurisdictionInfo>,
) -> Result<String, String> {
    let s3_client = make_s3_client().await;
    let mut processed_caselist = list_processed_cases_for_jurisdiction(&s3_client, &payload)
        .await
        .map_err(|e| e.to_string())?;
    // Randomizing the list just to insure that the processing difficulty is uniform.
    let mut rng = SmallRng::from_os_rng();
    processed_caselist.shuffle(&mut rng);
    let _res = download_attachments_from_docids(processed_caselist, &s3_client, &payload).await;
    Ok("Completed Successfully".into())
}

pub async fn download_attachments_from_docids(
    docid_list: Vec<String>,
    s3_client: &Client,
    jur_info: &JurisdictionInfo,
) {
    let extra_info = (s3_client.clone(), jur_info.clone());
    let _tasks = stream::iter(docid_list.into_iter())
        .map(|docket_govid| async {
            let docket_address = DocketAddress {
                jurisdiction: jur_info.clone(),
                docket_govid,
            };
            if let Ok(mut proc_docket) =
                download_openscrapers_object::<ProcessedGenericDocket>(s3_client, &docket_address)
                    .await
            {
                let res = proc_docket.download_incomplete(&extra_info).await;
                if res.is_ok() {
                    let _ = upload_object(s3_client, &docket_address, &proc_docket).await;
                }
            };
        })
        .buffer_unordered(20)
        .collect::<Vec<_>>()
        .await;
    info!(dockets_downloaded = %_tasks.len(),"Finished downloading attachments");
}
