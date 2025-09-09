use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use dokito_types::attachments::RawAttachment;
use mycorrhiza_common::tasks::{ExecuteUserTask, display_error_as_json};
use tokio::sync::{RwLock, RwLockReadGuard};

use crate::indexes::s3_storage_and_saving::{generate_attachment_url_index, pull_index_from_s3};

pub type AttachIndex = BTreeMap<String, RawAttachment>;

static GLOBAL_RAW_ATTACHMENT_URL_INDEX_CACHE: RwLock<AttachIndex> =
    RwLock::const_new(BTreeMap::new());

static HAS_PULLED_FROM_CACHE_ONCE: AtomicBool = AtomicBool::new(false);

pub async fn get_global_att_index() -> RwLockReadGuard<'static, AttachIndex> {
    if !HAS_PULLED_FROM_CACHE_ONCE.load(Ordering::Relaxed) {
        let new_index = pull_index_from_s3().await;
        let mut guard = GLOBAL_RAW_ATTACHMENT_URL_INDEX_CACHE.write().await;
        *guard = new_index;
        HAS_PULLED_FROM_CACHE_ONCE.store(true, Ordering::Relaxed);
    }

    GLOBAL_RAW_ATTACHMENT_URL_INDEX_CACHE.read().await
}
pub async fn regenrate_url_attach_index() -> anyhow::Result<()> {
    let attach_index = generate_attachment_url_index().await?;
    let mut guard = GLOBAL_RAW_ATTACHMENT_URL_INDEX_CACHE.write().await;
    *guard = attach_index;
    Ok(())
}

#[derive(Default, Clone, Copy)]
pub struct RegenerateUrlAttachIndex {}
#[async_trait]
impl ExecuteUserTask for RegenerateUrlAttachIndex {
    async fn execute_task(self: Box<Self>) -> Result<serde_json::Value, serde_json::Value> {
        let res = regenrate_url_attach_index().await;
        match res {
            Ok(_) => Ok("Task Succeeded".into()),
            Err(err) => Err(display_error_as_json(&err)),
        }
    }
    fn get_task_label_static() -> &'static str
    where
        Self: Sized,
    {
        "regenrate_url_attach_index"
    }
    fn get_task_label(&self) -> &'static str {
        "regenrate_url_attach_index"
    }
}

pub async fn lookup_hash_from_url(url: &str) -> Option<RawAttachment> {
    let index_guard = get_global_att_index().await;
    let result = index_guard.get(url);
    result.cloned()
}

use aide::{self, axum::IntoApiResponse};
use axum::{extract::Path, response::Json};
use schemars::JsonSchema;
use serde::Deserialize;
use url::Url;

#[derive(Deserialize, JsonSchema)]
pub struct UrlPath {
    /// The URL to lookup.
    pub url: String,
}

pub async fn handle_attachment_url_lookup(
    Path(UrlPath { url }): Path<UrlPath>,
) -> impl IntoApiResponse {
    match Url::parse(&url) {
        Ok(parsed_url) => {
            if let Some(attachment) = lookup_hash_from_url(parsed_url.as_str()).await {
                Ok(Json(attachment))
            } else {
                Err("URL not found in cache".to_string())
            }
        }
        Err(_) => Err("Invalid URL format".to_string()),
    }
}
