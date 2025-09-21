use std::{collections::HashSet, env, mem::take, sync::LazyLock};

use async_trait::async_trait;
use dokito_types::{
    env_vars::DIGITALOCEAN_S3,
    jurisdictions::JurisdictionInfo,
    processed::{OrgName, ProcessedGenericDocket},
    raw::RawGenericDocket,
    s3_stuff::{
        DocketAddress, list_processed_cases_for_jurisdiction, list_raw_cases_for_jurisdiction,
    },
};
use futures::stream::{self, StreamExt};
use rand::{SeedableRng, rngs::SmallRng, seq::SliceRandom};
use reqwest::Client;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use sqlx::{PgPool, Pool, Postgres, postgres::PgPoolOptions, types::Uuid};

use mycorrhiza_common::{
    s3_generic::cannonical_location::download_openscrapers_object, tasks::ExecuteUserTask,
};
use tracing::{info, warn};

use crate::{
    data_processing_traits::Revalidate, processing::process_case,
    sql_ingester_tasks::dokito_sql_connection::get_dokito_pool,
};

#[derive(Clone, Copy, Default, Deserialize, JsonSchema)]
pub struct NyPucIngestPurgePrevious {}

#[async_trait]
impl ExecuteUserTask for NyPucIngestPurgePrevious {
    async fn execute_task(self: Box<Self>) -> Result<Value, Value> {
        let res = get_all_ny_puc_data(true).await;
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

#[derive(Clone, Copy, Default, Deserialize, JsonSchema)]
pub struct NyPucIngestGetMissingDockets {}
#[async_trait]
impl ExecuteUserTask for NyPucIngestGetMissingDockets {
    async fn execute_task(self: Box<Self>) -> Result<Value, Value> {
        let res = get_all_ny_puc_data(false).await;
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

pub async fn get_all_ny_puc_data(purge_data: bool) -> anyhow::Result<()> {
    info!("Got request to ingest all nypuc data.");

    let pool = get_dokito_pool()?;
    info!("Created pg pool");

    // Drop all existing tables first
    if purge_data {
        delete_all_data(pool).await?;
        info!("Successfully deleted all old case data.");
    }
    // We can set this to always true since we just purged the dataset.
    let ignore_existing = true;
    // Get the list of case IDs
    let ny_jurisdiction = JurisdictionInfo::new_usa("ny_puc", "ny");
    let s3_client = DIGITALOCEAN_S3.make_s3_client().await;
    let mut case_govids: Vec<String> =
        list_raw_cases_for_jurisdiction(&s3_client, &ny_jurisdiction).await?;
    let original_caselist_length = case_govids.len();
    info!(length=%original_caselist_length,"Got list of all cases");

    if ignore_existing {
        let _ = filter_out_existing_dokito_cases(&pool, &mut case_govids).await;
    }

    let mut rng = SmallRng::from_os_rng();
    case_govids.shuffle(&mut rng);

    let cases_to_process_len = case_govids.len();
    info!(total_cases = %original_caselist_length, cases_to_process= %cases_to_process_len,"Filtered down original raw cases to a subset that is not present in the database.");

    let execute_case_wraped =
        async |case_id: String| ingest_wrapped_ny_data(&case_id, &pool, ignore_existing).await;

    // Create a stream of futures to fetch and ingest each case concurrently
    let futures_count = stream::iter(case_govids)
        .map(execute_case_wraped)
        .buffer_unordered(20)
        .count()
        .await;
    info!(
        futures_count,
        "Successfully completed all sql ingest futures."
    );
    info!(total_dockets = %original_caselist_length, missing_cases = % cases_to_process_len, attempted_cases = % futures_count,"Out of all the cases, we wanted to proccess the missing cases, and tried to process:");
    Ok(())
}

async fn filter_out_existing_dokito_cases(
    pool: &PgPool,
    govid_list: &mut Vec<String>,
) -> anyhow::Result<()> {
    let existing_db_govids = sqlx::query!("SELECT docket_govid FROM dockets")
        .map(|r| r.docket_govid)
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
            let extra_info = (s3_client, jurisdiction);
            let final_res = process_case(raw_case, &extra_info).await;
            final_res
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

async fn ingest_wrapped_ny_data(case_id: &str, pool: &PgPool, ignore_existing: bool) {
    let case_address = DocketAddress {
        jurisdiction: JurisdictionInfo::new_usa("ny_puc", "ny"),
        docket_govid: case_id.to_string(),
    };
    let case_res = get_processed_case_or_process_if_not_existing(&case_address).await;
    match case_res {
        Ok(case) => {
            const CASE_RETRIES: usize = 3;
            if let Err(e) =
                ingest_sql_case_with_retries(&case, pool, ignore_existing, CASE_RETRIES).await
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

pub async fn ingest_sql_case_with_retries(
    case: &ProcessedGenericDocket,
    pool: &Pool<Postgres>,
    ignore_existing: bool,
    tries: usize,
) -> anyhow::Result<()> {
    let mut return_res = Ok(());
    for remaining_tries in (0..tries).rev() {
        match ingest_sql_nypuc_case(case, pool, ignore_existing).await {
            Ok(val) => return Ok(val),
            Err(err) => {
                warn!(docket_govid=%case.case_govid, %remaining_tries,"Encountered error while processing docket, retrying.");
                return_res = Err(err);
                let existing_docket: Option<Uuid> = sqlx::query_scalar!(
                    "SELECT uuid FROM dockets WHERE docket_govid = $1",
                    &case.case_govid.as_str()
                )
                .fetch_optional(pool)
                .await?;

                if let Some(docket_uuid) = existing_docket {
                    sqlx::query!("DELETE FROM dockets WHERE uuid = $1", docket_uuid)
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

pub async fn ingest_sql_nypuc_case(
    case: &ProcessedGenericDocket,
    pool: &Pool<Postgres>,
    _ignore_existing: bool,
) -> anyhow::Result<()> {
    let petitioner_list: &[OrgName] = &case.petitioner_list;
    let petitioner_strings = petitioner_list
        .iter()
        .map(|n| n.name.to_string())
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
    let docket_uuid: Uuid = sqlx::query_scalar!(
        "INSERT INTO dockets (uuid, docket_govid, docket_description, docket_title, industry, hearing_officer, opened_date, closed_date, petitioner_strings, docket_type, docket_subtype )
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
         RETURNING uuid",
        case.object_uuid,
        &case.case_govid.as_str(),
        &case.description,
        &case.case_name,
        &case.industry,
        &case.hearing_officer,
        case.opened_date,
        case.closed_date,
        &petitioner_strings,
        case_type,
        case_subtype,
    )
    .fetch_one(pool)
    .await?;

    for petitioner in petitioner_list.iter() {
        let petitioner_uuid = fetch_or_insert_new_orgname(petitioner, pool).await?;
        sqlx::query!(
            "INSERT INTO docket_petitioned_by_org (docket_uuid, petitioner_uuid) VALUES ($1,$2)",
            docket_uuid,
            petitioner_uuid
        )
        .execute(pool)
        .await?;
    }

    for filling in case.filings.iter() {
        let individual_author_strings = filling
            .individual_authors
            .iter()
            .map(|s| s.name.to_string())
            .collect::<Vec<_>>();
        let organization_author_strings = filling
            .organization_authors
            .iter()
            .map(|s| s.name.to_string())
            .collect::<Vec<_>>();
        let filling_uuid: Uuid = sqlx::query_scalar!(
            "INSERT INTO fillings (uuid, docket_uuid, docket_govid, individual_author_strings, organization_author_strings, filed_date, filling_type, filling_name, filling_description, openscrapers_id)
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
             RETURNING uuid",
            filling.object_uuid,
            docket_uuid,
            &case.case_govid.as_str(),
            &individual_author_strings,
            &organization_author_strings,
            filling.filed_date,
            &filling.filing_type,
            &filling.name,
            &filling.description,
            &filling.object_uuid.to_string()
        )
        .fetch_one(pool)
        .await?;

        for attachment in filling.attachments.iter() {
            let hashstr = if let Some(hash) = attachment.hash {
                hash.to_string()
            } else {
                "".to_string()
            };
            sqlx::query!(
                "INSERT INTO attachments (uuid, parent_filling_uuid, blake2b_hash, attachment_file_extension, attachment_file_name, attachment_title, attachment_url, openscrapers_id)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                    ON CONFLICT (uuid) DO UPDATE SET
                    parent_filling_uuid = EXCLUDED.parent_filling_uuid,
                    blake2b_hash = EXCLUDED.blake2b_hash,
                    attachment_file_extension = EXCLUDED.attachment_file_extension,
                    attachment_file_name = EXCLUDED.attachment_file_name,
                    attachment_title = EXCLUDED.attachment_title,
                    attachment_url = EXCLUDED.attachment_url,
                    openscrapers_id = EXCLUDED.openscrapers_id
                    ",
                attachment.object_uuid,
                filling_uuid,
                hashstr,
                &attachment.document_extension.to_string(),
                &attachment.name,
                &attachment.name,
                &attachment.url,
                &attachment.object_uuid.to_string()
            ).execute(pool)
            .await?;
        }
    }

    tracing::info!(govid=%case.case_govid, uuid=%docket_uuid,"Successfully processed case with no errors");
}

async fn fetch_or_insert_new_orgname(
    org_author: &OrgName,
    pool: &Pool<Postgres>,
) -> Result<Uuid, anyhow::Error> {
    let org_author_str = org_author.name.as_str();
    let org_record  = sqlx::query!(
        "SELECT uuid, org_suffix FROM organizations WHERE name = $1 AND artifical_person_type = 'organization'",
        org_author_str,
    ).fetch_optional(pool)
    .await?;

    let org_uuid = if let Some(org_record) = org_record {
        if org_record.org_suffix.is_empty() && !org_author.suffix.is_empty() {
            let _ = sqlx::query!(
                "UPDATE organizations SET org_suffix = $1 WHERE uuid = $2",
                &org_author.suffix,
                &org_record.uuid
            )
            .execute(pool)
            .await?;
        };
        org_record.uuid
    } else {
        let org_suffix = &*org_author.suffix;
        let new_org: Uuid = sqlx::query_scalar!(
                    "INSERT INTO organizations (name, artifical_person_type, aliases, org_suffix) VALUES ($1, 'organization', $2, $3) RETURNING uuid",
                    org_author_str,
                    &vec![org_author_str.to_string()],
                    org_suffix,
                )
                .fetch_one(pool)
                .await?;
        new_org
    };
    Ok(org_uuid)
}

pub async fn delete_all_data(pool: &PgPool) -> anyhow::Result<()> {
    info!("Starting full data deletion...");

    // Start a transaction
    let mut tx = pool.begin().await?;

    // Disable statement timeout just for this transaction
    sqlx::query!("SET LOCAL statement_timeout = 0;")
        .execute(&mut *tx)
        .await?;
    info!("Disabled statement_timeout for this transaction");

    // Drop relation tables first (with CASCADE)
    info!("Deleting from fillings_filed_by_org_relation");
    sqlx::query!("TRUNCATE fillings_filed_by_org_relation CASCADE")
        .execute(&mut *tx)
        .await?;

    info!("Deleting from fillings_on_behalf_of_org_relation");
    sqlx::query!("TRUNCATE fillings_on_behalf_of_org_relation CASCADE")
        .execute(&mut *tx)
        .await?;

    // Attachments
    info!("Deleting from attachments");
    sqlx::query!("TRUNCATE attachments CASCADE")
        .execute(&mut *tx)
        .await?;

    // Organizations
    info!("Deleting from organizations");
    sqlx::query!("TRUNCATE organizations CASCADE")
        .execute(&mut *tx)
        .await?;

    // Fillings
    info!("Deleting from fillings");
    sqlx::query!("TRUNCATE fillings CASCADE")
        .execute(&mut *tx)
        .await?;

    // Dockets
    info!("Deleting from dockets");
    sqlx::query!("TRUNCATE dockets CASCADE")
        .execute(&mut *tx)
        .await?;

    // Commit once everything is successful
    tx.commit().await?;
    info!("All data deleted successfully ✅");

    Ok(())
}
