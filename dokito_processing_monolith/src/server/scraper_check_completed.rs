use axum::{Json, extract::Path};
use dokito_types::{deduplication::DoubleDeduplicated, s3_stuff::list_raw_cases_for_jurisdiction};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::info;

use crate::{
    jurisdiction_schema_mapping::FixedJurisdiction,
    s3_stuff::list_processed_cases_for_jurisdiction, server::s3_routes::JurisdictionPath,
    sql_ingester_tasks::dokito_sql_connection::get_dokito_pool,
    types::jurisdictions::JurisdictionInfo,
};

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
pub struct JuristdictionCaselistBreakdown {
    to_process: Vec<Value>,
    missing_completed: Vec<Value>,
    completed: Vec<Value>,
}

#[derive(Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LookupStage {
    Raw,
    Processed,
    Database,
}

#[derive(Deserialize, JsonSchema)]
pub struct JurisdictionPathWithLocation {
    /// The state of the jurisdiction.
    pub state: String,
    /// The name of the jurisdiction.
    pub jurisdiction_name: String,
    /// The location to look up cases from.
    pub lookup_stage: LookupStage,
}

pub async fn get_casedata_differential_with_location(
    Path(JurisdictionPathWithLocation {
        state,
        jurisdiction_name,
        lookup_stage,
    }): Path<JurisdictionPathWithLocation>,
    Json(caselist): Json<Vec<Value>>,
) -> Result<Json<JuristdictionCaselistBreakdown>, String> {
    let country = "usa".to_string();
    let jur_info = JurisdictionInfo {
        state,
        country,
        jurisdiction: jurisdiction_name,
    };
    let fixed_jur = FixedJurisdiction::try_from(&jur_info).map_err(|e| e.to_string())?;

    let result = generate_differential(caselist, fixed_jur, lookup_stage).await;
    match result {
        Ok(breakdown) => Ok(Json(breakdown)),
        Err(err) => Err(err.to_string()),
    }
}

pub async fn get_raw_caselist(
    Path(JurisdictionPathWithLocation {
        state,
        jurisdiction_name,
        lookup_stage: location,
    }): Path<JurisdictionPathWithLocation>,
) -> Result<Json<Vec<String>>, String> {
    let country = "usa".to_string();
    let jur_info = JurisdictionInfo {
        state,
        country,
        jurisdiction: jurisdiction_name,
    };
    let fixed_jur = FixedJurisdiction::try_from(&jur_info).map_err(|e| e.to_string())?;

    let result = fetch_complete_govid_list(fixed_jur, location).await;
    match result {
        Ok(caselist) => Ok(Json(caselist)),
        Err(err) => Err(err.to_string()),
    }
}

pub async fn generate_differential(
    data: Vec<serde_json::Value>,
    fixed_jur: FixedJurisdiction,
    source: LookupStage,
) -> Result<JuristdictionCaselistBreakdown, anyhow::Error> {
    let existing_ids = fetch_complete_govid_list(fixed_jur, source).await?;
    type ValueIdList = Vec<(String, Value)>;
    let existing_ids_values: ValueIdList = existing_ids
        .into_iter()
        .map(|id| (id.clone(), serde_json::Value::from(id)))
        .collect();

    let user_caselist_values = data
        .into_iter()
        .filter_map(|value| {
            let govid_docket = value.get("docket_govid").and_then(|v| v.as_str());
            let govid_with_case =
                govid_docket.or_else(|| value.get("case_govid").and_then(|v| v.as_str()));
            let govid = govid_with_case?;
            Some((govid.to_string(), value))
        })
        .collect::<Vec<_>>();

    let deduped = DoubleDeduplicated::make_double_deduplicated_with_keys(
        user_caselist_values,
        existing_ids_values,
    );
    let return_val = JuristdictionCaselistBreakdown {
        to_process: deduped.in_base,
        missing_completed: deduped.in_comparison,
        completed: deduped.in_both,
    };
    Ok(return_val)
}

pub async fn fetch_complete_govid_list(
    fixed_jur: FixedJurisdiction,
    source: LookupStage,
) -> Result<Vec<String>, anyhow::Error> {
    let s3_client = crate::s3_stuff::make_s3_client().await;

    let jur = JurisdictionInfo::from(fixed_jur);
    info!("Sucessfully created s3 client.");
    let result = match source {
        LookupStage::Processed => list_processed_cases_for_jurisdiction(&s3_client, &jur).await,
        LookupStage::Raw => list_raw_cases_for_jurisdiction(&s3_client, &jur).await,
        LookupStage::Database => list_database_cases_for_fixed_jur(fixed_jur).await,
    };
    info!("Completed call to s3 to get jurisdiction list.");
    result
}

pub async fn list_database_cases_for_fixed_jur(
    fixed_jur: FixedJurisdiction,
) -> Result<Vec<String>, anyhow::Error> {
    let pool = get_dokito_pool().await?;
    let pg_schema = fixed_jur.get_postgres_schema_name();

    let existing_db_govids: Vec<String> =
        sqlx::query_scalar(&format!("SELECT docket_govid FROM {pg_schema}.dockets"))
            .fetch_all(&*pool)
            .await?;

    Ok(existing_db_govids)
}
