//! Active-mode request handling: the s3s-facing side of the engine.
//!
//! Every function here returns `Result<Option<...>>`: `Ok(None)` and `Err`
//! both mean "fall back to raw passthrough" at the call site — the engine can
//! decline work, it can never fail a request (Invariant 0).

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use anyhow::Result;
use bytes::Bytes;
use futures::Stream;
use s3s::dto;
use tracing::warn;

use sparta_meter::record::{Mode, RecordBody};

use crate::engine::restore::RangeReq;
use crate::engine::{keys, now_ms, Engine};

/// mpsc receiver as a body stream for `StreamingBlob::wrap`.
struct RecvStream(tokio::sync::mpsc::Receiver<Result<Bytes, std::io::Error>>);

impl Stream for RecvStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

fn to_range_req(r: &dto::Range) -> RangeReq {
    match r {
        dto::Range::Int { first, last } => RangeReq::Int {
            first: *first,
            last: *last,
        },
        dto::Range::Suffix { length } => RangeReq::Suffix { length: *length },
    }
}

fn to_etag(s: &Option<String>) -> Option<dto::ETag> {
    let s = s.as_deref()?;
    s.parse()
        .ok()
        .or_else(|| format!("\"{}\"", s.trim_matches('"')).parse().ok())
}

fn ts_from_ms(ms: u64) -> dto::Timestamp {
    dto::Timestamp::from(std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms))
}

/// GET: serve from packs when a manifest exists.
pub async fn get_object(
    engine: &Arc<Engine>,
    input: &dto::GetObjectInput,
) -> Result<Option<dto::GetObjectOutput>> {
    if keys::is_engine_key(&input.key) {
        return Ok(None);
    }
    let range = input.range.as_ref().map(to_range_req);
    let Some(restored) = engine.restore(&input.bucket, &input.key, range).await? else {
        return Ok(None);
    };

    // Meter: logical bytes served + a lower bound on engine pack GETs.
    let distinct_packs = {
        let mut packs: Vec<&str> = restored
            .manifest
            .chunks
            .iter()
            .map(|c| c.pack_id.as_str())
            .collect();
        packs.sort_unstable();
        packs.dedup();
        packs.len() as u64
    };
    if let Err(e) = engine.meter.append(
        now_ms(),
        Mode::Active,
        RecordBody::Op {
            op: "get_object".into(),
            bucket: input.bucket.clone(),
            logical_bytes: restored.content_length,
            physical_bytes: restored.content_length,
            engine_put_requests: 0,
            engine_get_requests: distinct_packs,
            engine_bytes: 0,
            engine_cpu_us: 0,
        },
    ) {
        warn!(error = %e, "meter append failed for active GET");
    }

    let m = &restored.manifest;
    let output = dto::GetObjectOutput {
        body: Some(dto::StreamingBlob::wrap(RecvStream(restored.stream))),
        content_length: Some(restored.content_length as i64),
        content_type: m.content_type.clone(),
        e_tag: to_etag(&m.etag),
        last_modified: Some(ts_from_ms(m.created_unix_ms)),
        accept_ranges: Some("bytes".to_string()),
        content_range: restored
            .content_range
            .map(|(s, e, total)| format!("bytes {s}-{e}/{total}")),
        metadata: (!m.user_metadata.is_empty()).then(|| {
            m.user_metadata
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        }),
        ..Default::default()
    };
    Ok(Some(output))
}

/// HEAD from the manifest.
pub async fn head_object(
    engine: &Arc<Engine>,
    input: &dto::HeadObjectInput,
) -> Result<Option<dto::HeadObjectOutput>> {
    if keys::is_engine_key(&input.key) {
        return Ok(None);
    }
    let Some(m) = engine.fetch_manifest(&input.bucket, &input.key).await? else {
        return Ok(None);
    };
    if m.quarantined {
        return Ok(None);
    }
    Ok(Some(dto::HeadObjectOutput {
        content_length: Some(m.logical_size as i64),
        content_type: m.content_type.clone(),
        e_tag: to_etag(&m.etag),
        last_modified: Some(ts_from_ms(m.created_unix_ms)),
        accept_ranges: Some("bytes".to_string()),
        metadata: (!m.user_metadata.is_empty()).then(|| {
            m.user_metadata
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        }),
        ..Default::default()
    }))
}

/// After a passthrough write lands, queue the object for ingest.
pub async fn enqueue_written(engine: &Arc<Engine>, bucket: &str, key: &str, etag: Option<&str>) {
    if keys::is_engine_key(key) {
        return;
    }
    if let Err(e) = engine.db.enqueue_ingest(bucket, key, etag, now_ms()) {
        warn!(bucket, key, error = %e, "ingest enqueue failed; object stays raw");
        return;
    }
    engine.ingest_notify.notify_one();
}

/// DELETE: drop the engine version (manifest + queue entry). The caller still
/// proxies the raw delete afterwards.
pub async fn delete_engine_object(engine: &Arc<Engine>, bucket: &str, key: &str) {
    if keys::is_engine_key(key) {
        return;
    }
    let _guard = engine.lock_key(bucket, key).await;
    let kh = keys::keyhash(key);
    if let Err(e) = engine
        .delete_object_raw(bucket, &keys::manifest_key(&kh))
        .await
    {
        warn!(bucket, key, error = %e, "manifest delete failed (raw delete still proceeds)");
    }
    let _ = engine.db.delete_manifest(bucket, &kh);
    let _ = engine.db.delete_bands(bucket, &kh);
    let _ = engine.db.remove_ingest(bucket, key);
}

/// ListObjectsV2 over the merged view.
pub async fn list_objects_v2(
    engine: &Arc<Engine>,
    input: &dto::ListObjectsV2Input,
) -> Result<Option<dto::ListObjectsV2Output>> {
    let prefix = input.prefix.clone().unwrap_or_default();
    let page = engine
        .merged_list(
            &input.bucket,
            &prefix,
            input.delimiter.as_deref(),
            input.max_keys.unwrap_or(1000).max(1) as usize,
            input.continuation_token.as_deref(),
            input.start_after.as_deref(),
        )
        .await?;

    let mut contents = Vec::new();
    let mut common_prefixes = Vec::new();
    for e in page.entries {
        match e {
            crate::engine::listing::Entry::Object {
                key,
                size,
                etag,
                last_modified_ms,
            } => contents.push(dto::Object {
                key: Some(key),
                size: Some(size as i64),
                e_tag: to_etag(&etag),
                last_modified: last_modified_ms.map(ts_from_ms),
                storage_class: Some(dto::ObjectStorageClass::from(
                    dto::ObjectStorageClass::STANDARD.to_string(),
                )),
                ..Default::default()
            }),
            crate::engine::listing::Entry::CommonPrefix(p) => {
                common_prefixes.push(dto::CommonPrefix { prefix: Some(p) });
            }
        }
    }
    let key_count = (contents.len() + common_prefixes.len()) as i32;
    Ok(Some(dto::ListObjectsV2Output {
        name: Some(input.bucket.clone()),
        prefix: input.prefix.clone(),
        delimiter: input.delimiter.clone(),
        max_keys: input.max_keys,
        start_after: input.start_after.clone(),
        continuation_token: input.continuation_token.clone(),
        is_truncated: Some(page.next_token.is_some()),
        next_continuation_token: page.next_token,
        key_count: Some(key_count),
        contents: (!contents.is_empty()).then_some(contents),
        common_prefixes: (!common_prefixes.is_empty()).then_some(common_prefixes),
        ..Default::default()
    }))
}

/// CopyObject when the source is an engine object (or the dest becomes one).
///
/// Same-bucket engine source: clone the manifest (chunks shared — instant,
/// zero data movement). Cross-bucket engine source: materialize via restore
/// and write raw to the destination, then enqueue its ingest.
pub async fn copy_object(
    engine: &Arc<Engine>,
    input: &dto::CopyObjectInput,
) -> Result<Option<dto::CopyObjectOutput>> {
    let dto::CopySource::Bucket {
        bucket: src_bucket,
        key: src_key,
        ..
    } = &input.copy_source
    else {
        return Ok(None);
    };
    let src_bucket = src_bucket.to_string();
    let src_key = src_key.to_string();
    if keys::is_engine_key(&src_key) || keys::is_engine_key(&input.key) {
        return Ok(None);
    }
    let Some(src_manifest) = engine.fetch_manifest(&src_bucket, &src_key).await? else {
        // Raw source: proxy the copy; the caller enqueues dest ingest.
        return Ok(None);
    };
    if src_manifest.quarantined {
        return Ok(None);
    }

    if src_bucket == input.bucket {
        let _guard = engine.lock_key(&input.bucket, &input.key).await;
        let mut dest = src_manifest.clone();
        dest.key = input.key.clone();
        dest.created_unix_ms = now_ms();
        engine.store_manifest(&input.bucket, &dest).await?;
        // Shadowed leftovers under the dest key are stale now.
        let _ = engine.db.remove_ingest(&input.bucket, &input.key);
        let _ = engine.delete_object_raw(&input.bucket, &input.key).await;
        let _ = engine.meter.append(
            now_ms(),
            Mode::Active,
            RecordBody::Op {
                op: "copy_object".into(),
                bucket: input.bucket.clone(),
                logical_bytes: dest.logical_size,
                physical_bytes: 0, // chunks shared; no new stored bytes
                engine_put_requests: 1,
                engine_get_requests: 0,
                engine_bytes: 0,
                engine_cpu_us: 0,
            },
        );
        return Ok(Some(dto::CopyObjectOutput {
            copy_object_result: Some(dto::CopyObjectResult {
                e_tag: to_etag(&dest.etag),
                last_modified: Some(ts_from_ms(dest.created_unix_ms)),
                ..Default::default()
            }),
            ..Default::default()
        }));
    }

    // Cross-bucket: materialize (bounded by the ingest size cap).
    let Some(restored) = engine.restore(&src_bucket, &src_key, None).await? else {
        return Ok(None);
    };
    let mut stream = restored.stream;
    let mut body = Vec::with_capacity(restored.content_length as usize);
    while let Some(frame) = stream.recv().await {
        body.extend_from_slice(&frame?);
    }
    engine
        .put_object_bytes(&input.bucket, &input.key, body)
        .await?;
    enqueue_written(
        engine,
        &input.bucket,
        &input.key,
        src_manifest.etag.as_deref(),
    )
    .await;
    Ok(Some(dto::CopyObjectOutput {
        copy_object_result: Some(dto::CopyObjectResult {
            e_tag: to_etag(&src_manifest.etag),
            last_modified: Some(ts_from_ms(now_ms())),
            ..Default::default()
        }),
        ..Default::default()
    }))
}
