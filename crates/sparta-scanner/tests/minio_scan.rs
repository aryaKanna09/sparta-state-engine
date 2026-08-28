//! Scanner integration test against a live MinIO server.
//!
//! Run with `cargo test -p sparta-scanner -- --ignored` (CI does). Requires
//! the MinIO binary (SPARTA_TEST_MINIO or tools/bin/minio).

use std::process::Command;

use sparta_testkit::fixtures::FleetSpec;
use sparta_testkit::{fixtures, minio, seed};

fn scanner_bin() -> &'static str {
    env!("CARGO_BIN_EXE_sparta-scanner")
}

#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn scan_fleet_end_to_end() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let server = minio::MinioServer::start().expect("start minio");
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Seed a fleet: 3 templates x 45 snapshots of 8 MiB (spec scale — smaller
    // templates yield too few 1 MiB-average chunks for MinHash clustering),
    // gzip every 4th.
    rt.block_on(async {
        let client = seed::client_for(&server).await;
        seed::create_bucket(&client, "fleet").await.unwrap();
        let spec = FleetSpec {
            snapshots: 45,
            templates: 3,
            template_bytes: 8 * 1024 * 1024,
            ..Default::default()
        };
        let mut objs = Vec::new();
        fixtures::generate_fleet(&spec, |o| objs.push(o));
        // A DR copy prefix to exercise exclusion rules.
        let mut dr = Vec::new();
        fixtures::generate_fleet(
            &FleetSpec {
                snapshots: 6,
                templates: 1,
                template_bytes: 1024 * 1024,
                ..Default::default()
            },
            |mut o| {
                o.key = format!("dr-copies/{}", o.key);
                dr.push(o);
            },
        );
        objs.extend(dr);
        seed::put_objects(&client, "fleet", objs, 16).await.unwrap();
    });

    let out = tempfile::tempdir().unwrap();
    let out_dir = out.path().join("report");
    // Sample 100%: the fleet is small and the assertions need determinism.
    let status = Command::new(scanner_bin())
        .args([
            "--bucket",
            "fleet",
            "--endpoint",
            &server.endpoint,
            "--sample-rate",
            "1.0",
            "--exclude-prefix",
            "fleet/dr-copies/",
            "--out",
            out_dir.to_str().unwrap(),
        ])
        .env("AWS_ACCESS_KEY_ID", minio::ACCESS_KEY)
        .env("AWS_SECRET_ACCESS_KEY", minio::SECRET_KEY)
        .env("AWS_REGION", "us-east-1")
        .status()
        .expect("run scanner");
    assert!(status.success(), "scanner exited with {status}");

    let json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(out_dir.join("report.json")).expect("report.json exists"),
    )
    .expect("valid JSON report");

    let srf = json["srf_s"]["point"].as_f64().unwrap();
    assert!(
        srf >= 3.0,
        "fleet of 45 snapshots from 3 templates should show SRF-S >= 3, got {srf}"
    );
    let lo = json["srf_s"]["ci_low"].as_f64().unwrap();
    let hi = json["srf_s"]["ci_high"].as_f64().unwrap();
    assert!(lo <= srf && srf <= hi, "CI [{lo},{hi}] must bracket {srf}");

    // Normalization must have kicked in for the gzip variants.
    assert!(json["sample"]["normalized_objects"].as_u64().unwrap() > 0);
    // Exclusions counted.
    assert!(
        json["addressable"]["excluded_sampled_objects"]
            .as_u64()
            .unwrap()
            >= 6
    );
    // Prefix breakdown present.
    assert!(!json["by_prefix"].as_array().unwrap().is_empty());
    // Clusters: the three template families should surface.
    assert!(json["top_clusters"].as_array().unwrap().len() >= 2);

    // HTML report exists and is self-contained.
    let html = std::fs::read_to_string(out_dir.join("report.html")).unwrap();
    assert!(html.contains("SRF-S"));
    assert!(!html.contains("<script"));
}
