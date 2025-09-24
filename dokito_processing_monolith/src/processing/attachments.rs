use crate::data_processing_traits::{DownloadIncomplete, RevalidationOutcome};
use crate::indexes::attachment_url_index::lookup_hash_from_url;
use crate::jurisdiction_schema_mapping::FixedJurisdiction;
use crate::processing::file_fetching::{FileDownloadError, RequestMethod};
use crate::s3_stuff::{
    generate_s3_object_uri_from_key, get_raw_attach_file_key, get_s3_json_uri,
    push_raw_attach_file_to_s3, upload_object,
};
use crate::types::processed::ProcessedGenericAttachment;
use crate::types::{attachments::RawAttachment, jurisdictions::JurisdictionInfo};
use aws_sdk_s3::Client as S3Client;
use chrono::Utc;
use mycorrhiza_common::file_extension::FileExtension;
use mycorrhiza_common::hash::Blake2bHash;
use non_empty_string::{NonEmptyString, non_empty_string};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::sleep;
use tracing::{debug, info};

use super::file_fetching::{AdvancedFetchData, FileDownloadResult, InternetFileFetch};

const ATTACHMENT_DOWNLOAD_TRIES: usize = 2;
const DOWNLOAD_RETRY_DELAY_SECONDS: u64 = 2;

// For context it was previously this
// pub type OpenscrapersExtraData = (S3Client, JurisdictionInfo);

#[derive(Clone)]
pub struct OpenscrapersExtraData {
    pub s3_client: S3Client,
    pub jurisdiction_info: JurisdictionInfo,
    pub fixed_jurisdiction: FixedJurisdiction,
}

impl DownloadIncomplete for ProcessedGenericAttachment {
    type ExtraData = OpenscrapersExtraData;
    async fn download_incomplete(
        &mut self,
        extra_data: Self::ExtraData,
    ) -> anyhow::Result<RevalidationOutcome> {
        let name = NonEmptyString::from_str(&self.name)
            .unwrap_or_else(|_| non_empty_string!("unknown_filename"));
        if self.hash.is_some() {
            return Ok(RevalidationOutcome::NoChanges);
        }
        let res = lookup_hash_from_url(&self.url).await;
        if let Some(cached_attach) = res {
            self.hash = Some(cached_attach.hash);
            return Ok(RevalidationOutcome::DidChange);
        };
        debug!(url=%self.url,"Trying to download attachment file.");
        let extension = &self.document_extension;

        let FileDownloadResult {
            data: file_contents,
            filename: server_filename,
        } = download_file_content_validated_with_retries(&self.url, extension).await?;
        let hash = Blake2bHash::from_bytes(&file_contents);
        debug!(%hash, url=%self.url,"Successfully downloaded file.");

        let metadata = server_filename
            .map(|exant_filename| HashMap::from([("server_filename".to_string(), exant_filename)]))
            .unwrap_or_default();

        let raw_attachment = RawAttachment {
            jurisdiction_info: extra_data.jurisdiction_info.clone(),
            url: self.url.clone(),
            hash,
            file_size_bytes: file_contents.len() as u64,
            name,
            extension: extension.clone(),
            text_objects: vec![],
            date_added: Utc::now(),
            date_updated: Utc::now(),
            extra_metadata: metadata,
        };
        shipout_attachment_to_s3(file_contents, raw_attachment, &extra_data.s3_client).await?;
        self.hash = Some(hash);
        debug!(%hash, url = %self.url,"Successfully downloaded attachment and saved everything to s3.");
        Ok(RevalidationOutcome::DidChange)
    }
}

async fn shipout_attachment_to_s3(
    file_contents: Vec<u8>,
    mut raw_attachment: RawAttachment,
    s3_client: &S3Client,
) -> anyhow::Result<RawAttachment> {
    let hash = raw_attachment.hash;
    push_raw_attach_file_to_s3(s3_client, &raw_attachment, file_contents).await?;

    raw_attachment.date_updated = Utc::now();

    upload_object(s3_client, &hash, &raw_attachment).await?;

    Ok(raw_attachment)
}

#[derive(Serialize, Deserialize, Clone, JsonSchema, Debug)]
pub struct DirectAttachmentProcessInfo {
    pub file_name: Option<String>,
    pub extension: FileExtension,
    pub fetch_info: AdvancedFetchData,
    pub jurisdiction_info: JurisdictionInfo,
    pub wait_for_s3_upload: bool,
}

#[derive(Serialize, Deserialize, Clone, JsonSchema, Debug)]
pub struct DirectAttachmentReturnInfo {
    pub attachment: RawAttachment,
    pub hash: Blake2bHash,
    pub server_file_name: Option<String>,
    pub file_s3_uri: String,
    pub object_s3_uri: String,
}

pub async fn process_attachment_with_direct_request(
    direct_info: &DirectAttachmentProcessInfo,
    s3_client_owned: S3Client,
) -> anyhow::Result<DirectAttachmentReturnInfo> {
    let FileDownloadResult {
        data: file_contents,
        filename: server_filename,
    } = download_file_content_validated_with_retries(
        &direct_info.fetch_info,
        &direct_info.extension,
    )
    .await?;
    let hash = Blake2bHash::from_bytes(&file_contents);
    info!(%hash, fetch_info=?direct_info.fetch_info,"Successfully downloaded file.");

    let mut metadata = HashMap::new();
    if let Some(exant_filename) = server_filename.clone() {
        metadata.insert("server_file_name".to_string(), exant_filename);
    }
    let actual_filename = direct_info
        .file_name
        .clone()
        .or(server_filename.clone())
        .and_then(|x| NonEmptyString::new(x).ok())
        .unwrap_or(non_empty_string!("unknown"));

    let is_normal_url_request = direct_info.fetch_info.request_type == RequestMethod::Get
        && direct_info.fetch_info.request_body.is_none()
        && direct_info.fetch_info.headers.is_none();
    let url_value = match is_normal_url_request {
        true => Some(direct_info.fetch_info.url.clone()),
        false => None,
    };

    let raw_attachment = RawAttachment {
        jurisdiction_info: direct_info.jurisdiction_info.clone(),
        hash,
        file_size_bytes: file_contents.len() as u64,
        url: url_value.unwrap_or_default(),
        name: actual_filename.clone(),
        extension: direct_info.extension.clone(),
        text_objects: vec![],
        date_added: Utc::now(),
        date_updated: Utc::now(),
        extra_metadata: metadata,
    };
    let return_info = DirectAttachmentReturnInfo {
        attachment: raw_attachment.clone(),
        hash,
        server_file_name: server_filename,
        file_s3_uri: generate_s3_object_uri_from_key(&get_raw_attach_file_key(hash)),
        object_s3_uri: get_s3_json_uri::<RawAttachment>(&hash),
    };
    let mut return_info_clone = return_info.clone();
    let s3_process_future = async move || {
        let s3_client = &s3_client_owned;
        let new_attach = shipout_attachment_to_s3(file_contents, raw_attachment, s3_client).await?;
        return_info_clone.attachment = new_attach;
        Ok(return_info_clone)
    };
    match direct_info.wait_for_s3_upload {
        true => s3_process_future().await,
        false => {
            tokio::spawn(s3_process_future());
            Ok(return_info)
        }
    }
}

static MAXIMUM_EXTERNAL_FILE_DOWNLOADS: Semaphore = Semaphore::const_new(10);

async fn download_file_content_validated_with_retries<T: InternetFileFetch + ?Sized>(
    to_fetch: &T,
    extension: &FileExtension,
) -> Result<FileDownloadResult, FileDownloadError> {
    let permit = MAXIMUM_EXTERNAL_FILE_DOWNLOADS.acquire().await.unwrap();
    let mut last_error: Option<FileDownloadError> = None;
    for _ in 0..ATTACHMENT_DOWNLOAD_TRIES {
        match to_fetch
            .download_file_with_timeout(Duration::from_secs(20))
            .await
        {
            Ok(file_contents) => {
                if let Err(err) = extension.is_valid_file_contents(&file_contents.data) {
                    tracing::error!(%extension,?to_fetch, %err,"Downloaded file did not match extension");
                    last_error = Some(FileDownloadError::InvalidReturnData(err))
                } else {
                    return Ok(file_contents);
                }
            }
            Err(err) => {
                tracing::error!(?to_fetch, %err,"Encountered error downloading file");
                if !err.is_retryable() {
                    return Err(err);
                };
                last_error = Some(err);
            }
        };
        sleep(Duration::from_secs(DOWNLOAD_RETRY_DELAY_SECONDS)).await;
    }

    tracing::error!(%extension,?to_fetch,"Could not download file from url dispite a bunch of retries.");
    drop(permit);

    Err(last_error.unwrap())
}
