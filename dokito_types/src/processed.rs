use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use mycorrhiza_common::{file_extension::FileExtension, hash::Blake2bHash};
use non_empty_string::NonEmptyString;
use openscraper_types::raw::RawGenericParty;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, serde_as};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, JsonSchema, Clone)]
pub struct ProcessedGenericAttachment {
    #[serde(default)]
    pub name: String,
    pub index_in_filling: u64,
    pub document_extension: FileExtension,
    #[serde(default)]
    pub object_uuid: Uuid,
    #[serde(default)]
    pub attachment_govid: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub attachment_type: String,
    #[serde(default)]
    pub attachment_subtype: String,
    #[serde(default)]
    pub extra_metadata: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub hash: Option<Blake2bHash>,
}

#[derive(Serialize, Deserialize, Debug, JsonSchema, Clone)]
pub struct OrgName {
    pub name: NonEmptyString,
    pub suffix: String,
}

use serde::Deserializer;

// generic helper that turns either a Vec<T> or a map into a Vec<T>
fn deserialize_vec_or_map<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum VecOrMap<T> {
        Vec(Vec<T>),
        Map(HashMap<String, T>),
        NumKeyMap(HashMap<u64, T>),
    }

    match VecOrMap::<T>::deserialize(deserializer)? {
        VecOrMap::Vec(v) => Ok(v),
        VecOrMap::Map(m) => Ok(m.into_values().collect()),
        VecOrMap::NumKeyMap(m) => Ok(m.into_values().collect()),
    }
}

#[derive(Serialize, Deserialize, Debug, JsonSchema, Clone)]
#[serde(untagged)]
pub enum ProcessedArtificalPerson {
    Human(ProcessedGenericHuman),
    Organization(ProcessedGenericOrganization),
}

#[derive(Serialize, Deserialize, Debug, JsonSchema, Clone)]
struct ProcessedGenericHuman {
    pub human_name: NonEmptyString,
    #[serde(default)]
    pub object_uuid: Uuid,
    #[serde(default)]
    pub western_first_name: String,
    #[serde(default)]
    pub western_last_name: String,
    #[serde(default)]
    pub contact_emails: Vec<String>,
    #[serde(default)]
    pub contact_phone_numbers: Vec<String>,
    #[serde(default)]
    pub representing_company: Option<ProcessedGenericOrganization>,
    #[serde(default)]
    pub employed_by: Option<ProcessedGenericOrganization>,
    #[serde(default)]
    pub title: String,
}

#[derive(Serialize, Deserialize, Debug, JsonSchema, Clone)]
struct ProcessedGenericOrganization {
    pub truncated_org_name: NonEmptyString,
    #[serde(default)]
    pub org_suffix: String,
    #[serde(default)]
    pub object_uuid: Uuid,
    #[serde(default)]
    pub org_type: OrganizationType,
}

#[derive(Serialize, Deserialize, Debug, JsonSchema, Clone, Copy, Default)]
#[serde(rename_all = "snake_case")]
enum OrganizationType {
    #[default]
    Unknown,
    ForProfit,
    NonProfit,
    GovernmentAgency,
}

#[derive(Serialize, Deserialize, Debug, JsonSchema, Clone)]
pub struct ProcessedGenericFiling {
    pub filed_date: Option<NaiveDate>,
    pub index_in_docket: u64,
    #[serde(default)]
    pub filling_govid: String,
    #[serde(default)]
    pub filling_url: String,
    #[serde(default)]
    pub object_uuid: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub organization_authors: Vec<OrgName>,
    #[serde(default)]
    pub individual_authors: Vec<OrgName>,
    #[serde(default)]
    pub filing_type: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, deserialize_with = "deserialize_vec_or_map")]
    pub attachments: Vec<ProcessedGenericAttachment>, // 👈 handles both vec + map
    #[serde(default)]
    pub extra_metadata: HashMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, JsonSchema, Clone)]
pub struct ProcessedGenericDocket {
    pub case_govid: NonEmptyString,
    #[serde(default)]
    pub opened_date: NaiveDate,
    #[serde(default)]
    pub object_uuid: Uuid,
    #[serde(default)]
    pub case_name: String,
    #[serde(default)]
    pub case_url: String,
    #[serde(default)]
    pub case_type: String,
    #[serde(default)]
    pub case_subtype: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub industry: String,
    #[serde(default)]
    pub petitioner_list: Vec<OrgName>,
    #[serde(default)]
    pub hearing_officer: String,
    #[serde(default)]
    pub closed_date: Option<NaiveDate>,
    #[serde(default, deserialize_with = "deserialize_vec_or_map")]
    pub filings: Vec<ProcessedGenericFiling>, // 👈 same trick here
    #[serde(default)]
    pub case_parties: Vec<ProcessedArtificalPerson>,
    #[serde(default)]
    pub forwarded_raw_parties: Vec<RawGenericParty>,
    #[serde(default)]
    pub extra_metadata: HashMap<String, serde_json::Value>,
    #[serde(default = "Utc::now")]
    pub indexed_at: DateTime<Utc>,
    #[serde(default = "Utc::now")]
    pub processed_at: DateTime<Utc>,
}
