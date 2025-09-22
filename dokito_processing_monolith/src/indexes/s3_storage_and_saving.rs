use std::{collections::BTreeMap, path::Path, str::FromStr, sync::Arc};

use aws_sdk_s3::Client;
use dokito_types::{
    attachments::RawAttachment,
    env_vars::{DIGITALOCEAN_S3, OPENSCRAPERS_S3_OBJECT_BUCKET},
};
use mycorrhiza_common::{
    hash::Blake2bHash,
    s3_generic::{
        S3Credentials,
        cannonical_location::{CannonicalS3ObjectLocation, download_openscrapers_object},
        fetchers_and_getters::S3DirectoryAddr,
    },
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::indexes::attachment_url_index::AttachIndex;

async fn get_all_attachment_hashes(s3_client: &Client) -> anyhow::Result<Vec<Blake2bHash>> {
    let dir = "raw/metadata/";
    let bucket: &'static str = &OPENSCRAPERS_S3_OBJECT_BUCKET;
    let attach_folder = S3DirectoryAddr {
        s3_client,
        bucket,
        prefix: dir.into(),
    };
    let prefixes = attach_folder.list_all().await?;

    let mut hashes = Vec::with_capacity(prefixes.len());
    for prefix in prefixes {
        let path = Path::new(prefix.trim());
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let stripped_filekey = stem.strip_prefix(dir).unwrap_or(&stem);
        let stripped_filekey = stripped_filekey
            .strip_suffix(".json")
            .unwrap_or(stripped_filekey);
        if let Ok(hash) = Blake2bHash::from_str(stripped_filekey) {
            hashes.push(hash);
        } else {
            warn!(%stripped_filekey,"Encountered file name that could not be converted to a hash.")
        }
    }
    Ok(hashes)
}

pub async fn pull_index_from_s3() -> AttachIndex {
    let s3_client = DIGITALOCEAN_S3.make_s3_client().await;
    if let Ok(fetched_index) =
        download_openscrapers_object::<CanonAttachIndex>(&s3_client, &()).await
    {
        return fetched_index.0;
    };
    info!("Failed fetching from s3, setting index as empty.");

    BTreeMap::new()
}

// async fn generate_attachment_url_index() -> anyhow::Result<AttachIndex> {
//     let s3_client = Arc::new(DIGITALOCEAN_S3.make_s3_client().await);
//     let hashlist = get_all_attachment_hashes(&s3_client).await?;
//     let results = stream::iter(hashlist.iter())
//         .map(|hash| {
//             let s3_clone = s3_client.clone();
//             async move {
//                 let val = download_openscrapers_object::<RawAttachment>(&*s3_clone, hash).await;
//                 val
//             }
//         })
//         .buffer_unordered(20)
//         .collect::<Vec<_>>()
//         .await;
//     let map = results
//         .into_iter()
//         .filter_map(|r| match r {
//             Ok(att) => Some((att.url.clone(), att)),
//             Err(_err) => None,
//         })
//         .collect();
//     Ok(map)
// }

use tokio::sync::Semaphore;

pub async fn generate_attachment_url_index() -> anyhow::Result<AttachIndex> {
    info!("Starting attachment index generation");
    let s3_client = Arc::new(DIGITALOCEAN_S3.make_s3_client().await);
    let hashlist = get_all_attachment_hashes(&s3_client).await?;
    info!(hashlist_length = %hashlist.len(),"Got all hashes from directory.");

    // Limit concurrency to 20
    let semaphore = Arc::new(Semaphore::new(10));
    let mut handles = Vec::with_capacity(hashlist.len());

    for hash in hashlist {
        let s3_clone = s3_client.clone();
        let sem_clone = semaphore.clone();
        // Spawn each task
        let handle = tokio::spawn(async move {
            // Acquire a permit before starting
            let _permit = sem_clone.acquire().await.unwrap();
            let res = download_openscrapers_object::<RawAttachment>(&s3_clone, &hash).await;
            if let Err(e) = &res {
                warn!(%hash,error=%e,"Encountered error while processing hash")
            } else {
                info!(%hash,"Got attachment info successfully")
            };
            res
        });

        handles.push(handle);
    }

    // Collect unordered results
    let mut map = AttachIndex::new();
    for handle in handles {
        match handle.await? {
            Ok(att) => {
                map.insert(att.url.clone(), att);
            }
            Err(_err) => {
                // Ignore errors just like in your filter_map
            }
        }
    }

    Ok(map)
}

#[derive(Deserialize, Serialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct CanonAttachIndex(pub AttachIndex);

impl CannonicalS3ObjectLocation for CanonAttachIndex {
    type AddressInfo = ();
    fn generate_object_key(_: &Self::AddressInfo) -> String {
        "indexes/global/attachment_urls".to_string()
    }
    fn generate_bucket(_: &Self::AddressInfo) -> &'static str {
        &OPENSCRAPERS_S3_OBJECT_BUCKET
    }
    fn get_credentials(_: &Self::AddressInfo) -> &'static S3Credentials {
        &DIGITALOCEAN_S3
    }
}
