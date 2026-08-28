//! Thin read-only S3 wrapper. The scanner never writes to the bucket.

use anyhow::{Context, Result};
use aws_sdk_s3::Client;

use crate::types::ObjectMeta;

pub async fn build_client(endpoint: Option<&str>, region: Option<&str>) -> Client {
    let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest());
    if let Some(r) = region {
        loader = loader.region(aws_config::Region::new(r.to_string()));
    } else if endpoint.is_some() {
        // Custom endpoints (MinIO) don't care about region but the SDK
        // requires one.
        loader = loader.region(aws_config::Region::new("us-east-1"));
    }
    if let Some(e) = endpoint {
        loader = loader.endpoint_url(e.to_string());
    }
    let shared = loader.load().await;
    let mut builder = aws_sdk_s3::config::Builder::from(&shared);
    if endpoint.is_some() {
        // MinIO and most S3-compatibles need path-style addressing.
        builder = builder.force_path_style(true);
    }
    Client::from_conf(builder.build())
}

/// Enumerate a bucket via ListObjectsV2, feeding each page to `sink`.
pub async fn list_bucket(
    client: &Client,
    bucket: &str,
    mut sink: impl FnMut(ObjectMeta),
) -> Result<u64> {
    let mut token: Option<String> = None;
    let mut count = 0u64;
    loop {
        let mut req = client.list_objects_v2().bucket(bucket).max_keys(1000);
        if let Some(t) = &token {
            req = req.continuation_token(t);
        }
        let resp = req
            .send()
            .await
            .with_context(|| format!("ListObjectsV2 on {bucket}"))?;
        for obj in resp.contents() {
            let Some(key) = obj.key() else { continue };
            count += 1;
            sink(ObjectMeta {
                bucket: bucket.to_string(),
                key: key.to_string(),
                size: obj.size().unwrap_or(0).max(0) as u64,
                storage_class: obj.storage_class().map(|c| c.as_str().to_string()),
                excluded: false,
            });
        }
        if resp.is_truncated() == Some(true) {
            token = resp.next_continuation_token().map(str::to_string);
            if token.is_none() {
                break;
            }
        } else {
            break;
        }
    }
    Ok(count)
}

/// Fetch a whole object (no Range header).
pub async fn get_full(client: &Client, bucket: &str, key: &str) -> Result<GotObject> {
    get_inner(client, bucket, key, None).await
}

/// Fetch `[0, len)` of an object via a ranged GET.
pub async fn get_range(client: &Client, bucket: &str, key: &str, len: u64) -> Result<GotObject> {
    get_inner(
        client,
        bucket,
        key,
        Some(format!("bytes=0-{}", len.saturating_sub(1))),
    )
    .await
}

async fn get_inner(
    client: &Client,
    bucket: &str,
    key: &str,
    range: Option<String>,
) -> Result<GotObject> {
    let mut req = client.get_object().bucket(bucket).key(key);
    if let Some(r) = range {
        req = req.range(r);
    }
    let resp = req
        .send()
        .await
        .with_context(|| format!("GetObject {bucket}/{key}"))?;
    let content_type = resp.content_type().map(str::to_string);
    let body = resp
        .body
        .collect()
        .await
        .with_context(|| format!("read body {bucket}/{key}"))?
        .into_bytes();
    Ok(GotObject {
        bytes: body.to_vec(),
        content_type,
    })
}

pub struct GotObject {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
}

/// Fetch an object's tags as (key, value) pairs.
pub async fn get_tags(client: &Client, bucket: &str, key: &str) -> Result<Vec<(String, String)>> {
    let resp = client
        .get_object_tagging()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .with_context(|| format!("GetObjectTagging {bucket}/{key}"))?;
    Ok(resp
        .tag_set()
        .iter()
        .map(|t| (t.key().to_string(), t.value().to_string()))
        .collect())
}
