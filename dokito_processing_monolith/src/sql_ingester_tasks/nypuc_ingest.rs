use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash, Hasher},
    mem::take,
};

use async_trait::async_trait;
use dokito_types::{
    env_vars::DIGITALOCEAN_S3,
    jurisdictions::JurisdictionInfo,
    processed::{ProcessedGenericDocket, ProcessedGenericFiling, ProcessedGenericOrganization},
    raw::RawGenericDocket,
    s3_stuff::{DocketAddress, list_raw_cases_for_jurisdiction},
};
use futures::{
    future::join_all,
};
use rand::{SeedableRng, rngs::SmallRng, seq::SliceRandom};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use sqlx::{PgPool, Pool, Postgres, query_scalar, types::Uuid};

use mycorrhiza_common::{
    s3_generic::cannonical_location::{download_openscrapers_object, upload_object},
    tasks::ExecuteUserTask,
};
use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::{
    data_processing_traits::Revalidate,
    jurisdiction_schema_mapping::FixedJurisdiction,
    processing::{attachments::OpenscrapersExtraData, process_case},
    sql_ingester_tasks::{
        database_author_association::*, dokito_sql_connection::get_dokito_pool,
        recreate_dokito_table_schema::delete_all_data,
    },
};

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[repr(transparent)]
pub struct FixedJurisdictionPurgePrevious(pub FixedJurisdiction);
#[async_trait]
impl ExecuteUserTask for FixedJurisdictionPurgePrevious {
    async fn execute_task(self: Box<Self>) -> Result<Value, Value> {
        let res = ingest_all_fixed_jurisdiction_data(self.0, true).await;
        match res {
            Ok(()) => {
                info!("Nypuc ingest completed.");
                Ok("Task Completed Successfully".into())
            }
            Err(err) => {
                let err_debug = format!("{:?}", err);
                tracing::error!(error= % err, error_debug= &err_debug[..500],"Encountered error in ny_ingest");
                Err(err.to_string().into())
            }
        }
    }
    fn get_task_label(&self) -> &'static str {
        "ingest_nypuc_purge_previous"
    }
    fn get_task_label_static() -> &'static str
    where
        Self: Sized,
    {
        "ingest_nypuc_purge_previous"
    }
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[repr(transparent)]
pub struct GetMissingDocketsForFixedJurisdiction(pub FixedJurisdiction);
#[async_trait]
impl ExecuteUserTask for GetMissingDocketsForFixedJurisdiction {
    async fn execute_task(self: Box<Self>) -> Result<Value, Value> {
        let res = ingest_all_fixed_jurisdiction_data(self.0, false).await;
        match res {
            Ok(()) => {
                info!("Nypuc ingest completed.");
                Ok("Task Completed Successfully".into())
            }
            Err(err) => {
                let err_debug = format!("{:?}", err);
                tracing::error!(error= % err, error_debug= &err_debug[..500],"Encountered error in ny_ingest");
                Err(err.to_string().into())
            }
        }
    }
    fn get_task_label(&self) -> &'static str {
        "ingest_nypuc_get_missing_dockets"
    }
    fn get_task_label_static() -> &'static str
    where
        Self: Sized,
    {
        "ingest_nypuc_get_missing_dockets"
    }
}

pub async fn ingest_all_fixed_jurisdiction_data(
    fixed_jur: FixedJurisdiction,
    purge_data: bool,
) -> anyhow::Result<()> {
    info!("Got request to ingest all nypuc data.");

    let pool = get_dokito_pool()?;
    info!("Created pg pool");

    // Drop all existing tables first
    if purge_data {
        delete_all_data(fixed_jur, pool).await?;
        info!("Successfully deleted all old case data.");
    }
    // We can set this to always true since we just purged the dataset.
    let ignore_existing = true;
    // Get the list of case IDs
    let jurisdiction_info = JurisdictionInfo::from(fixed_jur);
    let s3_client = DIGITALOCEAN_S3.make_s3_client().await;
    let mut case_govids: Vec<String> =
        list_raw_cases_for_jurisdiction(&s3_client, &jurisdiction_info).await?;
    let original_caselist_length = case_govids.len();
    info!(length=%original_caselist_length,"Got list of all cases");

    if ignore_existing {
        let _ = filter_out_existing_dokito_cases(fixed_jur, pool, &mut case_govids).await;
    }

    let mut rng = SmallRng::from_os_rng();
    case_govids.shuffle(&mut rng);

    let cases_to_process_len = case_govids.len();
    info!(total_cases = %original_caselist_length, cases_to_process= %cases_to_process_len,"Filtered down original raw cases to a subset that is not present in the database.");

    let max_simultaneous_cases = Semaphore::new(20);
    let execute_case_wraped = async |case_id: String| {
        let _perm = max_simultaneous_cases.acquire().await;
        ingest_wrapped_fixed_jurisdiction_data(fixed_jur, &case_id, pool, ignore_existing).await
    };
    let future_cases = case_govids.into_iter().map(execute_case_wraped);
    let futures_count = join_all(future_cases).await.len();

    info!(
        futures_count,
        "Successfully completed all sql ingest futures."
    );
    info!(total_dockets = %original_caselist_length, missing_cases = % cases_to_process_len, attempted_cases = % futures_count,"Out of all the cases, we wanted to proccess the missing cases, and tried to process:");
    Ok(())
}

async fn filter_out_existing_dokito_cases(
    fixed_jur: FixedJurisdiction,
    pool: &PgPool,
    govid_list: &mut Vec<String>,
) -> anyhow::Result<()> {
    let pg_schema = fixed_jur.get_postgres_schema_name();
    let existing_db_govids: Vec<String> =
        query_scalar(&format!("SELECT docket_govid FROM {pg_schema}.dockets"))
            .fetch_all(pool)
            .await?;

    let case_govids_owned = take(govid_list);
    let mut case_govid_set = case_govids_owned.into_iter().collect::<HashSet<_>>();
    for existing_govid in existing_db_govids.iter() {
        case_govid_set.remove(existing_govid);
    }
    *govid_list = case_govid_set.into_iter().collect::<Vec<_>>();
    Ok(())
}

async fn get_processed_case_or_process_if_not_existing(
    case_address: &DocketAddress,
) -> anyhow::Result<ProcessedGenericDocket> {
    let s3_client = DIGITALOCEAN_S3.make_s3_client().await;
    let case_res =
        download_openscrapers_object::<ProcessedGenericDocket>(&s3_client, case_address).await;
    let docket = match case_res {
        Ok(docket) => Ok(docket),
        Err(_) => {
            let jurisdiction = case_address.jurisdiction.clone();
            let raw_case =
                download_openscrapers_object::<RawGenericDocket>(&s3_client, case_address).await?;
            let fixed_jurisdiction = FixedJurisdiction::try_from(&jurisdiction).unwrap();
            let extra_info = OpenscrapersExtraData {
                s3_client,
                jurisdiction_info: jurisdiction,
                fixed_jurisdiction,
            };

            process_case(raw_case, extra_info).await
        }
    };
    match docket {
        Ok(mut docket) => {
            docket.revalidate().await;
            Ok(docket)
        }
        Err(e) => Err(e),
    }
}

async fn ingest_wrapped_fixed_jurisdiction_data(
    fixed_jur: FixedJurisdiction,
    case_id: &str,
    pool: &PgPool,
    ignore_existing: bool,
) {
    let case_address = DocketAddress {
        jurisdiction: JurisdictionInfo::from(fixed_jur),
        docket_govid: case_id.to_string(),
    };
    let case_res = get_processed_case_or_process_if_not_existing(&case_address).await;
    match case_res {
        Ok(mut case) => {
            const CASE_RETRIES: usize = 3;
            if let Err(e) = ingest_sql_case_with_retries(
                &mut case,
                fixed_jur,
                pool,
                ignore_existing,
                CASE_RETRIES,
            )
            .await
            {
                let err_debug = format!("{:?}", e);
                tracing::error!(case_id = %case_id, error = %e, error_debug = &err_debug[..500], "Failed to ingest case, dispite retries.");
            }
        }
        Err(e) => {
            let err_debug = format!("{:?}", e);
            tracing::error!(case_id = %case_id, error = %e, error_debug = &err_debug[..500], "Failed to parse case")
        }
    }
}

fn generate_hash(x: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    x.hash(&mut hasher);
    hasher.finish()
}

pub async fn ingest_sql_case_with_retries(
    case: &mut ProcessedGenericDocket,
    fixed_jur: FixedJurisdiction,
    pool: &Pool<Postgres>,
    ignore_existing: bool,
    tries: usize,
) -> anyhow::Result<()> {
    let initial_hash = generate_hash(&*case);
    let mut return_res = Ok(());
    let pg_schema = fixed_jur.get_postgres_schema_name();
    for remaining_tries in (0..tries).rev() {
        match ingest_sql_fixed_jurisdiction_case(case, fixed_jur, pool, ignore_existing).await {
            Ok(val) => {
                let hash_post_upload = generate_hash(&*case);
                if hash_post_upload != initial_hash {
                    let s3_client = DIGITALOCEAN_S3.make_s3_client().await;
                    let addr = DocketAddress {
                        docket_govid: case.case_govid.to_string(),
                        jurisdiction: fixed_jur.into(),
                    };
                    // If this doesnt work everything should still be okay
                    let _ = upload_object(&s3_client, &addr, &*case).await;
                }
                return Ok(val);
            }
            Err(err) => {
                warn!(docket_govid=%case.case_govid, %remaining_tries,"Encountered error while processing docket, retrying.");
                return_res = Err(err);
                let existing_docket: Option<Uuid> = query_scalar(&format!(
                    "SELECT uuid FROM {pg_schema}.dockets WHERE docket_govid = $1"
                ))
                .bind(case.case_govid.as_str())
                .fetch_optional(pool)
                .await?;

                if let Some(docket_uuid) = existing_docket {
                    sqlx::query(&format!("DELETE FROM {pg_schema}.dockets WHERE uuid = $1"))
                        .bind(docket_uuid)
                        .execute(pool)
                        .await?;
                    info!(%docket_uuid, %case.case_govid,"Successfully deleted corrupted case data");
                } else {
                    info!(%case.case_govid,"Case ingest errored, but could not find corrupt case data.");
                }
            }
        }
    }
    return_res
}

pub fn bubble_error<T, E, I>(results: I) -> Result<(), E>
where
    I: IntoIterator<Item = Result<T, E>>,
{
    for res in results {
        res?; // early-return on the first Err
    }
    Ok(())
}
pub async fn ingest_sql_fixed_jurisdiction_case(
    case: &mut ProcessedGenericDocket,
    fixed_jur: FixedJurisdiction,
    pool: &Pool<Postgres>,
    _ignore_existing: bool,
) -> anyhow::Result<()> {
    let pg_schema = fixed_jur.get_postgres_schema_name();
    let petitioner_list: &mut [ProcessedGenericOrganization] = &mut case.petitioner_list;
    let petitioner_strings = petitioner_list
        .iter()
        .map(|n| n.truncated_org_name.to_string())
        .collect::<Vec<_>>();
    let mut case_type = case.case_type.clone();
    let mut case_subtype = case.case_subtype.clone();
    if let Some(actual_subtype_value) = case.extra_metadata.get("matter_subtype")
        && let Some(actual_subtype) = actual_subtype_value.as_str()
        && let Some(actual_type_value) = case.extra_metadata.get("matter_type")
        && let Some(actual_type) = actual_type_value.as_str()
    {
        case_type = actual_type.to_string();
        case_subtype = actual_subtype.to_string();
    }

    // Upsert docket
    let docket_uuid: Uuid = query_scalar(
        &format!("INSERT INTO {pg_schema}.dockets (uuid, docket_govid, docket_description, docket_title, industry, hearing_officer, opened_date, closed_date, petitioner_strings, docket_type, docket_subtype )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         ON CONFLICT (uuid) DO UPDATE SET
         docket_govid = EXCLUDED.docket_govid,
         docket_description = EXCLUDED.docket_description,
         docket_title = EXCLUDED.docket_title,
         industry = EXCLUDED.industry,
         hearing_officer = EXCLUDED.hearing_officer,
         opened_date = EXCLUDED.opened_date,
         closed_date = EXCLUDED.closed_date,
         petitioner_strings = EXCLUDED.petitioner_strings,
         docket_type = EXCLUDED.docket_type,
         docket_subtype = EXCLUDED.docket_subtype
         RETURNING uuid")
    )
    .bind(case.object_uuid)
    .bind(case.case_govid.as_str())
    .bind(&case.description)
    .bind(&case.case_name)
    .bind(&case.industry)
    .bind(&case.hearing_officer)
    .bind(case.opened_date)
    .bind(case.closed_date)
    .bind(&petitioner_strings)
    .bind(case_type)
    .bind(case_subtype)
    .fetch_one(pool)
    .await?;
    if docket_uuid != case.object_uuid {
        info!("Created new uuid for docket.")
    }

    let petitioner_futures = petitioner_list.iter_mut().map(async |petitioner| {
        upload_docket_petitioner_org_connection(petitioner, docket_uuid, fixed_jur, pool).await
    });
    let petitioner_results = join_all(petitioner_futures).await;
    bubble_error(petitioner_results.into_iter())?;

    let party_futures = case.case_parties.iter_mut().map(async |party| {
        upload_docket_party_human_connection(party, docket_uuid, fixed_jur, pool).await
    });
    let party_results = join_all(party_futures).await;
    bubble_error(party_results)?;

    let process_filling_closure =
        async |filling: &mut ProcessedGenericFiling| -> Result<(), anyhow::Error> {
            let individual_author_strings = filling
                .individual_authors
                .iter()
                .map(|s| s.human_name.to_string())
                .collect::<Vec<_>>();
            let organization_author_strings = filling
                .organization_authors
                .iter()
                .map(|s| s.truncated_org_name.to_string())
                .collect::<Vec<_>>();
            let filling_uuid: Uuid = query_scalar(
            &format!("INSERT INTO {pg_schema}.fillings (uuid, docket_uuid, docket_govid, individual_author_strings, organization_author_strings, filed_date, filling_type, filling_name, filling_description, openscrapers_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (uuid) DO UPDATE SET
             docket_uuid = EXCLUDED.docket_uuid,
             docket_govid = EXCLUDED.docket_govid,
             individual_author_strings = EXCLUDED.individual_author_strings,
             organization_author_strings = EXCLUDED.organization_author_strings,
             filed_date = EXCLUDED.filed_date,
             filling_type = EXCLUDED.filling_type,
             filling_name = EXCLUDED.filling_name,
             filling_description = EXCLUDED.filling_description,
             openscrapers_id = EXCLUDED.openscrapers_id
             RETURNING uuid")
        )
        .bind(filling.object_uuid)
        .bind(docket_uuid)
        .bind(case.case_govid.as_str())
        .bind(&individual_author_strings)
        .bind(&organization_author_strings)
        .bind(filling.filed_date)
        .bind(&filling.filing_type)
        .bind(&filling.name)
        .bind(&filling.description)
        .bind(filling.object_uuid.to_string())
        .fetch_one(pool)
        .await?;
            if filling_uuid != filling.object_uuid {
                info!(%filling_uuid, "Set filling to have new uuid");
                filling.object_uuid = filling_uuid;
            }

            // Associate individual authors using the proper association functions
            for individual_author in filling.individual_authors.iter_mut() {
                upload_filling_human_author(individual_author, filling_uuid, fixed_jur, pool)
                    .await?;
            }

            // Associate organization authors using the proper association functions
            for org_author in filling.organization_authors.iter_mut() {
                upload_filling_organization_author(org_author, filling_uuid, fixed_jur, pool)
                    .await?;
            }

            for attachment in filling.attachments.iter_mut() {
                let hashstr = attachment
                    .hash
                    .map(|h| h.to_string())
                    .unwrap_or_else(|| "".to_string());
                let attachment_uuid: Uuid = query_scalar(
                &format!("INSERT INTO {pg_schema}.attachments (uuid, parent_filling_uuid, blake2b_hash, attachment_file_extension, attachment_file_name, attachment_title, attachment_url, openscrapers_id)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                    ON CONFLICT (uuid) DO UPDATE SET
                    parent_filling_uuid = EXCLUDED.parent_filling_uuid,
                    blake2b_hash = EXCLUDED.blake2b_hash,
                    attachment_file_extension = EXCLUDED.attachment_file_extension,
                    attachment_file_name = EXCLUDED.attachment_file_name,
                    attachment_title = EXCLUDED.attachment_title,
                    attachment_url = EXCLUDED.attachment_url,
                    openscrapers_id = EXCLUDED.openscrapers_id
                    RETURNING uuid")
            )
            .bind(attachment.object_uuid)
            .bind(filling_uuid)
            .bind(hashstr)
            .bind(&*attachment.document_extension.to_string())
            .bind(&attachment.name)
            .bind(&attachment.name)
            .bind(&attachment.url)
            .bind(&*attachment.object_uuid.to_string())
            .fetch_one(pool)
            .await?;
                if attachment_uuid != attachment.object_uuid {
                    info!(%attachment_uuid, "Set attachment to have new uuid");
                    attachment.object_uuid = attachment_uuid;
                }
            }
            Ok(())
        };
    let simultaneous_file_uploads = Semaphore::new(10);
    let filling_futures = case.filings.iter_mut().map(async |filling| {
        let _permit = simultaneous_file_uploads.acquire().await?;
        process_filling_closure(filling).await
    });
    let filling_results = join_all(filling_futures).await;
    bubble_error(filling_results.into_iter())?;

    tracing::info!(govid=%case.case_govid, uuid=%docket_uuid,"Successfully processed case with no errors");
    Ok(())
}
