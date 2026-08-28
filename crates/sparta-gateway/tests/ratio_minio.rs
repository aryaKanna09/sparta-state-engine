//! M5 acceptance: checkpoint-aware chunking, delta chains, TrailStore.
//!
//! Run with `cargo test -p sparta-gateway --test ratio_minio -- --ignored`.

mod common;

use std::time::{Duration, Instant};

use common::Gateway;
use sparta_testkit::{fixtures, minio, seed};

fn spawn_active(upstream: &str, extra: &[(&str, &str)]) -> Gateway {
    let mut envs = vec![("SPARTA_MODE", "active"), ("SPARTA_GC_GRACE_DAYS", "0")];
    envs.extend_from_slice(extra);
    let gw = Gateway::spawn(upstream, &envs);
    gw.wait_ready();
    gw
}

async fn wait_ingested(upstream: &aws_sdk_s3::Client, bucket: &str, key: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if upstream
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .is_err()
        {
            return;
        }
        assert!(Instant::now() < deadline, "{bucket}/{key} never ingested");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn prefix_bytes(client: &aws_sdk_s3::Client, bucket: &str, prefix: &str) -> u64 {
    let mut total = 0u64;
    let mut token: Option<String> = None;
    loop {
        let mut req = client.list_objects_v2().bucket(bucket).prefix(prefix);
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

async fn fetch_manifest_json(
    upstream: &aws_sdk_s3::Client,
    bucket: &str,
    object_key: &str,
) -> serde_json::Value {
    let kh = hex::encode(blake3::hash(object_key.as_bytes()).as_bytes());
    let mkey = format!("_sparta/manifests/{kh}.json");
    let bytes = upstream
        .get_object()
        .bucket(bucket)
        .key(&mkey)
        .send()
        .await
        .unwrap_or_else(|e| panic!("manifest {mkey} missing: {e}"))
        .body
        .collect()
        .await
        .unwrap()
        .into_bytes();
    serde_json::from_slice(&bytes).expect("manifest json")
}

/// The M5 headline: two adjacent RL checkpoints, 98% bit-identical tensors —
/// the second must add < 5% of its file size in stored pack bytes.
#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn adjacent_rl_checkpoints_delta_under_5_percent() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = minio::MinioServer::start().expect("start minio");
    let gw = spawn_active(&server.endpoint, &[]);
    let via_gw = rt.block_on(seed::client_for_endpoint(&gw.endpoint));
    let upstream = rt.block_on(seed::client_for(&server));
    rt.block_on(seed::create_bucket(&via_gw, "ckpt")).unwrap();

    // 50 tensors x 1 MiB, 1 changed (2%): "98 percent bit identical tensors".
    let (ckpt_a, ckpt_b) = fixtures::generate_rl_checkpoints(50, 1024 * 1024, 1, 0xC4C4);
    assert_eq!(ckpt_a.len(), ckpt_b.len());

    rt.block_on(async {
        via_gw
            .put_object()
            .bucket("ckpt")
            .key("run/step-001000.safetensors")
            .body(aws_sdk_s3::primitives::ByteStream::from(ckpt_a.clone()))
            .send()
            .await
            .unwrap();
        wait_ingested(&upstream, "ckpt", "run/step-001000.safetensors").await;
        let packs_after_a = prefix_bytes(&upstream, "ckpt", "_sparta/packs/").await;

        // Declare lineage explicitly (the header route).
        via_gw
            .put_object()
            .bucket("ckpt")
            .key("run/step-001100.safetensors")
            .metadata("sparta-parent", "run/step-001000.safetensors")
            .body(aws_sdk_s3::primitives::ByteStream::from(ckpt_b.clone()))
            .send()
            .await
            .unwrap();
        wait_ingested(&upstream, "ckpt", "run/step-001100.safetensors").await;
        let packs_after_b = prefix_bytes(&upstream, "ckpt", "_sparta/packs/").await;

        let delta = packs_after_b.saturating_sub(packs_after_a);
        let frac = delta as f64 / ckpt_b.len() as f64;
        eprintln!(
            "checkpoint delta stored: {} bytes = {:.2}% of {} (packs {} -> {})",
            delta,
            frac * 100.0,
            ckpt_b.len(),
            packs_after_a,
            packs_after_b
        );
        assert!(
            frac < 0.05,
            "M5 acceptance: delta stored must be <5% of file size, got {:.2}%",
            frac * 100.0
        );

        // Lineage recorded in the manifest graph.
        let manifest = fetch_manifest_json(&upstream, "ckpt", "run/step-001100.safetensors").await;
        assert_eq!(
            manifest["parent"].as_str(),
            Some("run/step-001000.safetensors"),
            "lineage must be recorded"
        );

        // And both checkpoints restore byte-identically.
        for (key, want) in [
            ("run/step-001000.safetensors", &ckpt_a),
            ("run/step-001100.safetensors", &ckpt_b),
        ] {
            let got = via_gw
                .get_object()
                .bucket("ckpt")
                .key(key)
                .send()
                .await
                .unwrap()
                .body
                .collect()
                .await
                .unwrap()
                .into_bytes();
            assert_eq!(got.as_ref(), want.as_slice(), "byte identity for {key}");
        }
    });
}

/// MinHash-detected parent (no header) + delta-coded chunks that survive the
/// full read path.
#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn minhash_parent_detection_and_delta_roundtrip() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = minio::MinioServer::start().expect("start minio");
    // Sibling chunk-set Jaccard for ~16-chunk objects sits well under the
    // conservative 0.7 default; the test pins an explicit threshold.
    let gw = spawn_active(
        &server.endpoint,
        &[("SPARTA_DELTA_MINHASH_THRESHOLD", "0.4")],
    );
    let via_gw = rt.block_on(seed::client_for_endpoint(&gw.endpoint));
    let upstream = rt.block_on(seed::client_for(&server));
    rt.block_on(seed::create_bucket(&via_gw, "delta")).unwrap();

    // Parent: a snapshot image. Child: the same image re-snapshotted after a
    // small localized write — high MinHash similarity, a few changed chunks.
    let mut fleet = Vec::new();
    fixtures::generate_fleet(
        &fixtures::FleetSpec {
            snapshots: 1,
            templates: 1,
            template_bytes: 16 * 1024 * 1024,
            gzip_every: 0,
            ..Default::default()
        },
        |o| fleet.push(o),
    );
    let parent_bytes = fleet[0].bytes.clone();
    let mut child_bytes = parent_bytes.clone();
    for b in &mut child_bytes[5_000_000..5_065_536] {
        *b ^= 0xA5;
    }
    child_bytes.extend_from_slice(b"run 42 step 999 reward 0.4242\n");

    rt.block_on(async {
        via_gw
            .put_object()
            .bucket("delta")
            .key("snaps/parent.tar")
            .body(aws_sdk_s3::primitives::ByteStream::from(
                parent_bytes.clone(),
            ))
            .send()
            .await
            .unwrap();
        wait_ingested(&upstream, "delta", "snaps/parent.tar").await;

        // No header: detection must go through MinHash bands.
        via_gw
            .put_object()
            .bucket("delta")
            .key("snaps/child.tar")
            .body(aws_sdk_s3::primitives::ByteStream::from(
                child_bytes.clone(),
            ))
            .send()
            .await
            .unwrap();
        wait_ingested(&upstream, "delta", "snaps/child.tar").await;

        let manifest = fetch_manifest_json(&upstream, "delta", "snaps/child.tar").await;
        assert_eq!(
            manifest["parent"].as_str(),
            Some("snaps/parent.tar"),
            "MinHash detection must record lineage"
        );

        let got = via_gw
            .get_object()
            .bucket("delta")
            .key("snaps/child.tar")
            .send()
            .await
            .unwrap()
            .body
            .collect()
            .await
            .unwrap()
            .into_bytes();
        assert_eq!(
            got.as_ref(),
            child_bytes.as_slice(),
            "delta child byte identity"
        );
    });
}

/// TrailStore: train a dictionary, then verify future writes under the prefix
/// use it and still restore byte-identically; tier packs by age rule.
#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn train_dict_and_tiering() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = minio::MinioServer::start().expect("start minio");
    let gw = spawn_active(&server.endpoint, &[]);
    let via_gw = rt.block_on(seed::client_for_endpoint(&gw.endpoint));
    let upstream = rt.block_on(seed::client_for(&server));
    rt.block_on(seed::create_bucket(&via_gw, "corpus")).unwrap();

    // Seed a corpus of structurally similar trajectory files.
    let mk_doc = |i: u64| -> Vec<u8> {
        let mut out = Vec::new();
        for step in 0..2400u64 {
            out.extend_from_slice(
                format!(
                    "{{\"run\":{i},\"step\":{step},\"action\":\"move_{}\",\"reward\":{}.{:03},\"state_digest\":\"{:016x}\"}}\n",
                    step % 7,
                    (i * step) % 9,
                    step % 997,
                    i.wrapping_mul(step | 1)
                )
                .as_bytes(),
            );
        }
        out
    };
    rt.block_on(async {
        for i in 0..8u64 {
            via_gw
                .put_object()
                .bucket("corpus")
                .key(format!("trajectories/run-{i:03}.jsonl"))
                .body(aws_sdk_s3::primitives::ByteStream::from(mk_doc(i)))
                .send()
                .await
                .unwrap();
        }
        for i in 0..8u64 {
            wait_ingested(
                &upstream,
                "corpus",
                &format!("trajectories/run-{i:03}.jsonl"),
            )
            .await;
        }
    });

    // Train the dictionary via the CLI against the same DB the gateway uses.
    let db = gw.db_path.to_str().unwrap().to_string();
    let meter = gw.meter_path.to_str().unwrap().to_string();
    let envs = [
        ("SPARTA_UPSTREAM_ENDPOINT", server.endpoint.as_str()),
        ("SPARTA_UPSTREAM_ACCESS_KEY", minio::ACCESS_KEY),
        ("SPARTA_UPSTREAM_SECRET_KEY", minio::SECRET_KEY),
        ("SPARTA_MODE", "active"),
        ("SPARTA_DB_PATH", db.as_str()),
        ("SPARTA_METER_PATH", meter.as_str()),
    ];
    let out = common::gateway_cli(
        &[
            "train-dict",
            "--bucket",
            "corpus",
            "--prefix",
            "trajectories/",
        ],
        &envs,
    );
    assert!(
        out.status.success(),
        "train-dict failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(report["dict_bytes"].as_u64().unwrap() > 0);

    // New writes under the prefix pick up the trained dictionary (the rule is
    // read from the shared DB) and must restore byte-identically.
    let doc = mk_doc(99);
    rt.block_on(async {
        via_gw
            .put_object()
            .bucket("corpus")
            .key("trajectories/run-099.jsonl")
            .body(aws_sdk_s3::primitives::ByteStream::from(doc.clone()))
            .send()
            .await
            .unwrap();
        wait_ingested(&upstream, "corpus", "trajectories/run-099.jsonl").await;
        let got = via_gw
            .get_object()
            .bucket("corpus")
            .key("trajectories/run-099.jsonl")
            .send()
            .await
            .unwrap()
            .body
            .collect()
            .await
            .unwrap()
            .into_bytes();
        assert_eq!(
            got.as_ref(),
            doc.as_slice(),
            "dict-coded object byte identity"
        );
    });

    // Tiering: rule matches everything under trajectories/, age 0 days.
    let mut tier_envs = envs.to_vec();
    tier_envs.push(("SPARTA_TIER_RULES", "trajectories/=0:REDUCED_REDUNDANCY"));
    let out = common::gateway_cli(&["tier", "--bucket", "corpus", "--dry-run"], &tier_envs);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let dry: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        !dry["transitions"].as_array().unwrap().is_empty(),
        "dry run must plan transitions: {dry}"
    );
    let out = common::gateway_cli(&["tier", "--bucket", "corpus"], &tier_envs);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let real: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        !real["transitions"].as_array().unwrap().is_empty(),
        "tier run must transition engine packs: {real}"
    );

    // Objects still read fine after the storage-class transition.
    rt.block_on(async {
        let got = via_gw
            .get_object()
            .bucket("corpus")
            .key("trajectories/run-000.jsonl")
            .send()
            .await
            .unwrap()
            .body
            .collect()
            .await
            .unwrap()
            .into_bytes();
        assert_eq!(
            got.as_ref(),
            mk_doc(0).as_slice(),
            "post-tier byte identity"
        );
    });
}
