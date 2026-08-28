//! Bucket seeding helpers: create a bucket and load fixtures concurrently.

use anyhow::{Context, Result};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use futures::stream::{FuturesUnordered, StreamExt};

use crate::fixtures::FixtureObject;
use crate::minio::{MinioServer, ACCESS_KEY, SECRET_KEY};

/// S3 client aimed at a harness MinIO server.
pub async fn client_for(server: &MinioServer) -> Client {
    client_for_endpoint(&server.endpoint).await
}

pub async fn client_for_endpoint(endpoint: &str) -> Client {
    // Standard AWS env wins (compose/CI); testkit dev creds as fallback.
    let ak = std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| ACCESS_KEY.to_string());
    let sk = std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| SECRET_KEY.to_string());
    let creds = aws_sdk_s3::config::Credentials::new(ak, sk, None, None, "sparta-testkit");
    let shared = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(creds)
        .endpoint_url(endpoint)
        .load()
        .await;
    let conf = aws_sdk_s3::config::Builder::from(&shared)
        .force_path_style(true)
        .build();
    Client::from_conf(conf)
}

pub async fn create_bucket(client: &Client, bucket: &str) -> Result<()> {
    match client.create_bucket().bucket(bucket).send().await {
        Ok(_) => Ok(()),
        Err(e) => {
            let msg = format!("{e}");
            // Idempotent: already-owned buckets are fine.
            if msg.contains("BucketAlreadyOwnedByYou") || msg.contains("BucketAlreadyExists") {
                Ok(())
            } else {
                Err(e).with_context(|| format!("create bucket {bucket}"))
            }
        }
    }
}

/// PUT a batch of fixture objects with bounded concurrency.
pub async fn put_objects(
    client: &Client,
    bucket: &str,
    objects: Vec<FixtureObject>,
    concurrency: usize,
) -> Result<u64> {
    let mut inflight = FuturesUnordered::new();
    let mut count = 0u64;
    for obj in objects {
        let client = client.clone();
        let bucket = bucket.to_string();
        inflight.push(async move {
            client
                .put_object()
                .bucket(&bucket)
                .key(&obj.key)
                .content_type(obj.content_type)
                .body(ByteStream::from(obj.bytes))
                .send()
                .await
                .with_context(|| format!("put {bucket}/{}", obj.key))
                .map(|_| ())
        });
        if inflight.len() >= concurrency {
            inflight.next().await.transpose()?;
            count += 1;
        }
    }
    while let Some(r) = inflight.next().await {
        r?;
        count += 1;
    }
    Ok(count)
}
