use std::{collections::BTreeMap, str::FromStr};

use aws_sdk_s3::Client;
use dokito_types::{
    attachments::RawAttachment,
    env_vars::{DIGITALOCEAN_S3, DIGITALOCEAN_S3_OBJECT_BUCKET},
};
use futures::{StreamExt, stream};
use mycorrhiza_common::{
    hash::Blake2bHash,
    s3_generic::{
        S3Credentials,
        cannonical_location::{CannonicalS3ObjectLocation, download_openscrapers_object},
        fetchers_and_getters::S3DirectoryAddr,
    },
};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::indexes::attachment_url_index::AttachIndex;

async fn get_all_attachment_hashes(s3_client: &Client) -> anyhow::Result<Vec<Blake2bHash>> {
    let dir = "raw/metadata";
    let bucket: &'static str = &DIGITALOCEAN_S3_OBJECT_BUCKET;
    let attach_folder = S3DirectoryAddr {
        s3_client: s3_client,
        bucket,
        prefix: dir.into(),
    };
    let prefixes = attach_folder.list_all().await?;

    let mut hashes = Vec::with_capacity(prefixes.len());
    for prefix in prefixes {
        let stripped = prefix.strip_suffix(".json").unwrap_or(&prefix);
        if let Ok(hash) = Blake2bHash::from_str(stripped) {
            hashes.push(hash);
        } else {
            warn!(%prefix,"Encountered file name that could not be converted to a hash.")
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
    if let Ok(generated_index) = generate_attachment_url_index().await {
        return generated_index;
    }

    BTreeMap::new()
}

async fn generate_attachment_url_index() -> anyhow::Result<AttachIndex> {
    let s3_client = DIGITALOCEAN_S3.make_s3_client().await;
    let s3_client_ref = &s3_client;
    let hashlist = get_all_attachment_hashes(&s3_client).await?;
    let results = stream::iter(hashlist.iter())
        .map(|hash| async move {
            
            download_openscrapers_object::<RawAttachment>(s3_client_ref, hash).await
        })
        .buffer_unordered(20)
        .collect::<Vec<_>>()
        .await;
    let map = results
        .into_iter()
        .filter_map(|r| match r {
            Ok(att) => Some((att.url.clone(), att)),
            Err(_err) => None,
        })
        .collect();
    Ok(map)
}

#[derive(Deserialize, Serialize)]
pub struct CanonAttachIndex(pub AttachIndex);

impl CannonicalS3ObjectLocation for CanonAttachIndex {
    type AddressInfo = ();
    fn generate_object_key(addr: &Self::AddressInfo) -> String {
        "indexes/global/attachment_urls".to_string()
    }
    fn generate_bucket(addr: &Self::AddressInfo) -> &'static str {
        &DIGITALOCEAN_S3_OBJECT_BUCKET
    }
    fn get_credentials(addr: &Self::AddressInfo) -> &'static S3Credentials {
        &DIGITALOCEAN_S3
    }
}
