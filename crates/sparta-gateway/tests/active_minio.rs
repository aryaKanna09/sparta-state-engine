//! M4 acceptance: active-mode write-side dedup against live MinIO.
//!
//! Run with `cargo test -p sparta-gateway --test active_minio -- --ignored`.

mod common;

use std::time::{Duration, Instant};

use common::{test_payload, Gateway};
use sparta_testkit::fixtures::{self, FleetSpec};
use sparta_testkit::{minio, seed};

fn active_env(tmp_cache: &tempfile::TempDir) -> Vec<(String, String)> {
    vec![
        ("SPARTA_MODE".into(), "active".into()),
        (
            "SPARTA_CACHE_DIR".into(),
            tmp_cache.path().join("cache").to_str().unwrap().into(),
        ),
        ("SPARTA_GC_GRACE_DAYS".into(), "0".into()),
    ]
}

fn spawn_active(upstream: &str, cache: &tempfile::TempDir) -> Gateway {
    let envs = active_env(cache);
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let gw = Gateway::spawn(upstream, &env_refs);
    gw.wait_ready();
    gw
}

/// Wait until the write-through ingest replaced the raw with a manifest.
async fn wait_ingested(upstream: &aws_sdk_s3::Client, bucket: &str, key: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let raw = upstream
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .is_ok();
        if !raw {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "object {bucket}/{key} was never ingested (raw still present)"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn engine_bytes(upstream: &aws_sdk_s3::Client, bucket: &str, prefix: &str) -> u64 {
    let mut total = 0u64;
    let mut token: Option<String> = None;
    loop {
        let mut req = upstream.list_objects_v2().bucket(bucket).prefix(prefix);
        if let Some(t) = &token {
            req = req.continuation_token(t);
        }
        let resp = req.send().await.unwrap();
        for o in resp.contents() {
            total += o.size().unwrap_or(0).max(0) as u64;
        }
        if resp.is_truncated() == Some(true) {
            token = resp.next_continuation_token().map(str::to_string);
        } else {
            return total;
        }
    }
}

#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn active_write_read_lifecycle() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = minio::MinioServer::start().expect("start minio");
    let cache = tempfile::tempdir().unwrap();
    let gw = spawn_active(&server.endpoint, &cache);

    let via_gw = rt.block_on(seed::client_for_endpoint(&gw.endpoint));
    let upstream = rt.block_on(seed::client_for(&server));
    rt.block_on(seed::create_bucket(&via_gw, "fleet")).unwrap();

    let body = test_payload(9, 0xAC);
    let gzipped = fixtures::gzip(&test_payload(6, 0x91));

    rt.block_on(async {
        // PUT through the gateway (passthrough + enqueue).
        via_gw
            .put_object()
            .bucket("fleet")
            .key("runs/a.bin")
            .content_type("application/octet-stream")
            .metadata("run", "42")
            .body(aws_sdk_s3::primitives::ByteStream::from(body.clone()))
            .send()
            .await
            .expect("PUT a.bin");
        via_gw
            .put_object()
            .bucket("fleet")
            .key("runs/b.tar.gz")
            .body(aws_sdk_s3::primitives::ByteStream::from(gzipped.clone()))
            .send()
            .await
            .expect("PUT b.tar.gz");

        // Both must be readable immediately (raw window).
        let got = via_gw
            .get_object()
            .bucket("fleet")
            .key("runs/a.bin")
            .send()
            .await
            .unwrap()
            .body
            .collect()
            .await
            .unwrap()
            .into_bytes();
        assert_eq!(got.as_ref(), body.as_slice(), "pre-ingest read");

        // Ingest happens in the background.
        wait_ingested(&upstream, "fleet", "runs/a.bin").await;
        wait_ingested(&upstream, "fleet", "runs/b.tar.gz").await;

        // Engine artifacts exist; raw objects are gone.
        assert!(engine_bytes(&upstream, "fleet", "_sparta/packs/").await > 0);
        assert!(engine_bytes(&upstream, "fleet", "_sparta/manifests/").await > 0);

        // GET byte identity from packs (BLAKE3-verified server-side).
        let got = via_gw
            .get_object()
            .bucket("fleet")
            .key("runs/a.bin")
            .send()
            .await
            .expect("GET after ingest")
            .body
            .collect()
            .await
            .unwrap()
            .into_bytes();
        assert_eq!(got.as_ref(), body.as_slice(), "post-ingest read");

        // The gzip object goes through the recompression recipe path.
        let got = via_gw
            .get_object()
            .bucket("fleet")
            .key("runs/b.tar.gz")
            .send()
            .await
            .expect("GET gz after ingest")
            .body
            .collect()
            .await
            .unwrap()
            .into_bytes();
        assert_eq!(
            got.as_ref(),
            gzipped.as_slice(),
            "recipe path byte identity"
        );

        // Ranged GET.
        let rng = via_gw
            .get_object()
            .bucket("fleet")
            .key("runs/a.bin")
            .range("bytes=1048576-2097151")
            .send()
            .await
            .expect("ranged GET");
        assert_eq!(
            rng.content_range().unwrap_or_default(),
            format!("bytes 1048576-2097151/{}", body.len())
        );
        let part = rng.body.collect().await.unwrap().into_bytes();
        assert_eq!(part.as_ref(), &body[1048576..2097152], "range bytes");

        // HEAD from the manifest.
        let head = via_gw
            .head_object()
            .bucket("fleet")
            .key("runs/a.bin")
            .send()
            .await
            .expect("HEAD");
        assert_eq!(head.content_length().unwrap(), body.len() as i64);
        assert_eq!(
            head.metadata().and_then(|m| m.get("run").cloned()),
            Some("42".to_string()),
            "user metadata must survive ingest"
        );

        // Merged listing shows engine objects, hides _sparta/.
        let listed = via_gw
            .list_objects_v2()
            .bucket("fleet")
            .send()
            .await
            .expect("list");
        let listed_keys: Vec<&str> = listed.contents().iter().filter_map(|o| o.key()).collect();
        assert!(listed_keys.contains(&"runs/a.bin"), "{listed_keys:?}");
        assert!(listed_keys.contains(&"runs/b.tar.gz"));
        assert!(
            !listed_keys.iter().any(|k| k.starts_with("_sparta/")),
            "_sparta/ must be hidden: {listed_keys:?}"
        );
        // Listed sizes must be the logical sizes.
        let a = listed
            .contents()
            .iter()
            .find(|o| o.key() == Some("runs/a.bin"))
            .unwrap();
        assert_eq!(a.size().unwrap(), body.len() as i64);

        // Delimiter grouping over the merged view.
        let grouped = via_gw
            .list_objects_v2()
            .bucket("fleet")
            .delimiter("/")
            .send()
            .await
            .expect("list with delimiter");
        let prefixes: Vec<&str> = grouped
            .common_prefixes()
            .iter()
            .filter_map(|p| p.prefix())
            .collect();
        assert_eq!(prefixes, vec!["runs/"], "grouped: {prefixes:?}");

        // Same-bucket copy: manifest clone, chunks shared.
        via_gw
            .copy_object()
            .bucket("fleet")
            .key("runs/a-copy.bin")
            .copy_source("fleet/runs/a.bin")
            .send()
            .await
            .expect("copy engine object");
        let got = via_gw
            .get_object()
            .bucket("fleet")
            .key("runs/a-copy.bin")
            .send()
            .await
            .expect("GET copy")
            .body
            .collect()
            .await
            .unwrap()
            .into_bytes();
        assert_eq!(got.as_ref(), body.as_slice(), "copy byte identity");

        // Delete removes the engine object everywhere.
        via_gw
            .delete_object()
            .bucket("fleet")
            .key("runs/a.bin")
            .send()
            .await
            .expect("delete");
        let err = via_gw
            .get_object()
            .bucket("fleet")
            .key("runs/a.bin")
            .send()
            .await;
        assert!(err.is_err(), "deleted object must 404");
        let listed = via_gw
            .list_objects_v2()
            .bucket("fleet")
            .prefix("runs/")
            .send()
            .await
            .unwrap();
        let keys: Vec<&str> = listed.contents().iter().filter_map(|o| o.key()).collect();
        assert!(!keys.contains(&"runs/a.bin"), "{keys:?}");
        assert!(
            keys.contains(&"runs/a-copy.bin"),
            "copy must survive: {keys:?}"
        );
    });
}

#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn fleet_srf_benchmark_and_rebuild_and_gc() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = minio::MinioServer::start().expect("start minio");
    let cache = tempfile::tempdir().unwrap();
    let gw = spawn_active(&server.endpoint, &cache);

    let via_gw = rt.block_on(seed::client_for_endpoint(&gw.endpoint));
    let upstream = rt.block_on(seed::client_for(&server));
    rt.block_on(seed::create_bucket(&via_gw, "fleet")).unwrap();

    // The spec fixture: 200 snapshots from 3 base templates, raw + gzip.
    let spec = FleetSpec::default();
    let mut objs = Vec::new();
    fixtures::generate_fleet(&spec, |o| objs.push(o));
    let logical_bytes: u64 = objs.iter().map(|o| o.bytes.len() as u64).sum();
    let keys: Vec<String> = objs.iter().map(|o| o.key.clone()).collect();
    let sample: Vec<(String, Vec<u8>)> = objs
        .iter()
        .step_by(37)
        .map(|o| (o.key.clone(), o.bytes.clone()))
        .collect();

    eprintln!(
        "seeding {} snapshots ({:.1} MiB logical) through the gateway",
        objs.len(),
        logical_bytes as f64 / (1024.0 * 1024.0)
    );
    rt.block_on(seed::put_objects(&via_gw, "fleet", objs, 8))
        .unwrap();

    // Wait for every object to be ingested.
    rt.block_on(async {
        for key in &keys {
            wait_ingested(&upstream, "fleet", key).await;
        }
    });

    // --- SRF through the engine ----------------------------------------
    let physical = rt.block_on(async {
        engine_bytes(&upstream, "fleet", "_sparta/packs/").await
            + engine_bytes(&upstream, "fleet", "_sparta/manifests/").await
    });
    let srf = logical_bytes as f64 / physical as f64;
    eprintln!(
        "fleet SRF through engine: {:.2}x ({} logical / {} physical)",
        srf, logical_bytes, physical
    );
    assert!(
        srf >= 5.0,
        "M4 acceptance requires SRF >= 5, measured {srf:.2}"
    );

    // Byte identity on random sample reads.
    rt.block_on(async {
        for (key, want) in &sample {
            let got = via_gw
                .get_object()
                .bucket("fleet")
                .key(key)
                .send()
                .await
                .unwrap_or_else(|e| panic!("GET {key}: {e}"))
                .body
                .collect()
                .await
                .unwrap()
                .into_bytes();
            assert_eq!(got.as_ref(), want.as_slice(), "byte identity for {key}");
        }
    });

    // --- rebuild-index: delete the DB, restore full read + list ---------
    let envs = [
        ("SPARTA_UPSTREAM_ENDPOINT", server.endpoint.as_str()),
        ("SPARTA_UPSTREAM_ACCESS_KEY", minio::ACCESS_KEY),
        ("SPARTA_UPSTREAM_SECRET_KEY", minio::SECRET_KEY),
        ("SPARTA_MODE", "active"),
    ];
    let tmp = gw.shutdown();
    std::fs::remove_file(tmp.path().join("gw.sqlite")).expect("delete local DB");
    let _ = std::fs::remove_file(tmp.path().join("gw.sqlite-wal"));
    let _ = std::fs::remove_file(tmp.path().join("gw.sqlite-shm"));

    let db_path = tmp.path().join("gw.sqlite");
    let mut cli_envs = envs.to_vec();
    let db_str = db_path.to_str().unwrap().to_string();
    let meter_str = tmp.path().join("meter.jsonl").to_str().unwrap().to_string();
    cli_envs.push(("SPARTA_DB_PATH", &db_str));
    cli_envs.push(("SPARTA_METER_PATH", &meter_str));
    let out = common::gateway_cli(&["rebuild-index", "--bucket", "fleet"], &cli_envs);
    assert!(
        out.status.success(),
        "rebuild-index failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["manifests_restored"].as_u64().unwrap(), 200);
    assert!(report["chunks_indexed"].as_u64().unwrap() > 0);

    // Restart the gateway on the rebuilt DB: reads and listings must work.
    let gw = Gateway::spawn_with_dirs(
        &server.endpoint,
        &[("SPARTA_MODE", "active"), ("SPARTA_GC_GRACE_DAYS", "0")],
        tmp,
    );
    gw.wait_ready();
    let via_gw = rt.block_on(seed::client_for_endpoint(&gw.endpoint));
    rt.block_on(async {
        for (key, want) in &sample {
            let got = via_gw
                .get_object()
                .bucket("fleet")
                .key(key)
                .send()
                .await
                .unwrap_or_else(|e| panic!("GET {key} after rebuild: {e}"))
                .body
                .collect()
                .await
                .unwrap()
                .into_bytes();
            assert_eq!(got.as_ref(), want.as_slice(), "post-rebuild identity {key}");
        }
        let listed = via_gw
            .list_objects_v2()
            .bucket("fleet")
            .prefix("snapshots/")
            .max_keys(1000)
            .send()
            .await
            .unwrap();
        assert!(
            listed.key_count().unwrap_or(0) >= 200,
            "post-rebuild listing must show the fleet, got {:?}",
            listed.key_count()
        );
    });

    // --- GC: delete a template family, dry-run, then real two-phase -----
    rt.block_on(async {
        for key in keys.iter().filter(|k| k.contains("/tpl2/")) {
            via_gw
                .delete_object()
                .bucket("fleet")
                .key(key)
                .send()
                .await
                .unwrap();
        }
    });
    let packs_before = rt.block_on(engine_bytes(&upstream, "fleet", "_sparta/packs/"));

    let mut cli_envs2 = envs.to_vec();
    cli_envs2.push(("SPARTA_DB_PATH", &db_str));
    cli_envs2.push(("SPARTA_METER_PATH", &meter_str));
    cli_envs2.push(("SPARTA_GC_GRACE_DAYS", "0"));
    cli_envs2.push(("SPARTA_GC_COMPACT_THRESHOLD", "0.2"));

    // Dry run: reports what compaction and the sweep would do, changes nothing.
    let out = common::gateway_cli(&["gc", "--bucket", "fleet", "--dry-run"], &cli_envs2);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let dry: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let would_act = !dry["packs_compacted"].as_array().unwrap().is_empty()
        || !dry["packs_marked"].as_array().unwrap().is_empty();
    assert!(
        would_act,
        "deleting a template family must strand chunks somewhere: {dry}"
    );
    let after_dry = rt.block_on(engine_bytes(&upstream, "fleet", "_sparta/packs/"));
    assert_eq!(
        packs_before, after_dry,
        "dry run must not change the bucket"
    );

    // Real run 1: compacts garbage-heavy packs (old packs now unreferenced)
    // and marks candidates. Real run 2: grace(0) elapsed -> deletes them.
    let out = common::gateway_cli(&["gc", "--bucket", "fleet"], &cli_envs2);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let first: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let out = common::gateway_cli(&["gc", "--bucket", "fleet"], &cli_envs2);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let swept: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        !swept["packs_deleted"].as_array().unwrap().is_empty(),
        "second sweep must delete stranded packs (first run: {first}, second: {swept})"
    );
    let after_gc = rt.block_on(engine_bytes(&upstream, "fleet", "_sparta/packs/"));
    assert!(
        after_gc < packs_before,
        "pack bytes must shrink after GC ({packs_before} -> {after_gc})"
    );

    // Surviving objects still read byte-identically after GC.
    rt.block_on(async {
        for (key, want) in sample.iter().filter(|(k, _)| !k.contains("/tpl2/")) {
            let got = via_gw
                .get_object()
                .bucket("fleet")
                .key(key)
                .send()
                .await
                .unwrap_or_else(|e| panic!("GET {key} after GC: {e}"))
                .body
                .collect()
                .await
                .unwrap()
                .into_bytes();
            assert_eq!(got.as_ref(), want.as_slice(), "post-GC identity {key}");
        }
    });
}

#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn boto3_workflow_in_active_mode() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let server = minio::MinioServer::start().expect("start minio");
    let cache = tempfile::tempdir().unwrap();
    let gw = spawn_active(&server.endpoint, &cache);

    let python = common::workspace_root().join("tools/venv/bin/python");
    let python = if python.is_file() {
        python
    } else {
        "python3".into()
    };
    let script = common::workspace_root().join("crates/sparta-testkit/clients/boto3_workflow.py");
    let size_mb = std::env::var("SPARTA_E2E_SIZE_MB").unwrap_or_else(|_| "256".into());
    let status = std::process::Command::new(python)
        .arg(script)
        .args(["--endpoint", &gw.endpoint, "--size-mb", &size_mb])
        .env("AWS_ACCESS_KEY_ID", minio::ACCESS_KEY)
        .env("AWS_SECRET_ACCESS_KEY", minio::SECRET_KEY)
        .env("AWS_DEFAULT_REGION", "us-east-1")
        .status()
        .expect("run boto3 workflow");
    assert!(status.success(), "boto3 workflow failed in active mode");
}

#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn active_fault_dead_db_and_panicking_chunker() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = minio::MinioServer::start().expect("start minio");
    let cache = tempfile::tempdir().unwrap();
    let mut envs = active_env(&cache);
    envs.push(("SPARTA_FAULT_DB_FAIL".into(), "1".into()));
    envs.push(("SPARTA_FAULT_PANIC_TAP".into(), "1".into()));
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let gw = Gateway::spawn(&server.endpoint, &env_refs);
    gw.wait_ready();

    let client = rt.block_on(seed::client_for_endpoint(&gw.endpoint));
    rt.block_on(seed::create_bucket(&client, "faults")).unwrap();
    let body = test_payload(8, 0xAF);
    rt.block_on(async {
        client
            .put_object()
            .bucket("faults")
            .key("obj.bin")
            .body(aws_sdk_s3::primitives::ByteStream::from(body.clone()))
            .send()
            .await
            .expect("PUT with dead DB in active mode");
        let got = client
            .get_object()
            .bucket("faults")
            .key("obj.bin")
            .send()
            .await
            .expect("GET with dead DB in active mode")
            .body
            .collect()
            .await
            .unwrap()
            .into_bytes();
        assert_eq!(got.as_ref(), body.as_slice(), "byte identity under faults");
    });
}
