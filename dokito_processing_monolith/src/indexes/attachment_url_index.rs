use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};

use dokito_types::attachments::RawAttachment;
use tokio::sync::{RwLock, RwLockReadGuard};

use crate::indexes::s3_storage_and_saving::pull_index_from_s3;

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
    let guard = GLOBAL_RAW_ATTACHMENT_URL_INDEX_CACHE.read().await;
    guard
}

pub async fn lookup_hash_from_url(url: &str) -> Option<RawAttachment> {
    let index_guard = get_global_att_index().await;
    let result = index_guard.get(url);
    result.cloned()
}
