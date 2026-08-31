//! Gateway integration tests against live MinIO: shadow-mode workflows via
//! unmodified clients, and the Invariant 0 fault injection suite.
//!
//! Run with `cargo test -p sparta-gateway -- --ignored` (CI does). Requires
//! the MinIO binary (SPARTA_TEST_MINIO or tools/bin/minio).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use sparta_testkit::faultproxy::{Fault, FaultProxy};
use sparta_testkit::{minio, seed};

fn gateway_bin() -> &'static str {
    env!("CARGO_BIN_EXE_sparta-gateway")
}

fn meter_bin() -> PathBuf {
    // `cargo test` only uplifts a package's normal binary when that package
    // has an integration-test target of its own; sparta-meter has none, so on
    // a fresh checkout target/<profile>/sparta-meter does not exist when this
    // test runs. Build it on demand instead of relying on a stale artifact.
    let path = PathBuf::from(
        env!("CARGO_BIN_EXE_sparta-gateway").replace("sparta-gateway", "sparta-meter"),
    );
    if !path.is_file() {
        let mut cmd = Command::new(env!("CARGO"));
        cmd.args(["build", "-p", "sparta-meter", "--bins"]);
        if path.parent().is_some_and(|p| p.ends_with("release")) {
            cmd.arg("--release");
        }
        let status = cmd.status().expect("spawn cargo build -p sparta-meter");
        assert!(status.success(), "cargo build -p sparta-meter failed");
    }
    path
}

fn workspace_root() -> PathBuf {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    d.pop();
    d.pop();
    d
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http_status(port: u16, path: &str) -> Option<u16> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut buf = [0u8; 128];
    let n = stream.read(&mut buf).ok()?;
    let head = String::from_utf8_lossy(&buf[..n]);
    head.split_whitespace().nth(1)?.parse().ok()
}

struct Gateway {
    child: Child,
    pub port: u16,
    pub endpoint: String,
    _tmp: tempfile::TempDir,
    pub db_path: PathBuf,
    pub meter_path: PathBuf,
}

impl Gateway {
    fn spawn(upstream: &str, extra_env: &[(&str, &str)]) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("gw.sqlite");
        let meter_path = tmp.path().join("meter.jsonl");
        let port = free_port();
        let mut cmd = Command::new(gateway_bin());
        cmd.arg("serve")
            .env("SPARTA_LISTEN", format!("127.0.0.1:{port}"))
            .env("SPARTA_UPSTREAM_ENDPOINT", upstream)
            .env("SPARTA_ACCESS_KEY", minio::ACCESS_KEY)
            .env("SPARTA_SECRET_KEY", minio::SECRET_KEY)
            .env("SPARTA_DB_PATH", &db_path)
            .env("SPARTA_METER_PATH", &meter_path)
            .env("SPARTA_METER_LEVEL_INTERVAL_S", "1")
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        let child = cmd.spawn().expect("spawn gateway");
        let gw = Self {
            child,
            port,
            endpoint: format!("http://127.0.0.1:{port}"),
            _tmp: tmp,
            db_path,
            meter_path,
        };
        gw.wait_healthy();
        gw
    }

    fn wait_healthy(&self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if http_status(self.port, "/healthz") == Some(200) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("gateway did not become healthy");
    }

    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if http_status(self.port, "/readyz") == Some(200) {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("gateway did not become ready (upstream probe)");
    }

    fn stats(&self) -> serde_json::Value {
        let out = Command::new(gateway_bin())
            .arg("stats")
            .env("SPARTA_DB_PATH", &self.db_path)
            .env("SPARTA_UPSTREAM_ENDPOINT", "http://unused:1")
            .output()
            .expect("gateway stats");
        assert!(out.status.success(), "stats failed: {out:?}");
        serde_json::from_slice(&out.stdout).expect("stats JSON")
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn test_payload(mb: usize, seed_v: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(mb << 20);
    let mut s = seed_v | 1;
    while out.len() < mb << 20 {
        if (out.len() >> 20) % 3 == 0 {
            out.extend_from_slice(b"sparta gateway fault suite payload ");
        } else {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            out.extend_from_slice(&s.to_le_bytes());
        }
    }
    out.truncate(mb << 20);
    out
}

async fn roundtrip(client: &aws_sdk_s3::Client, bucket: &str, key: &str, body: &[u8]) {
    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(aws_sdk_s3::primitives::ByteStream::from(body.to_vec()))
        .send()
        .await
        .unwrap_or_else(|e| panic!("PUT {key}: {e}"));
    let got = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {key}: {e}"))
        .body
        .collect()
        .await
        .expect("collect body")
        .into_bytes();
    assert_eq!(
        got.as_ref(),
        body,
        "byte identity through gateway for {key}"
    );
}

// ---------------------------------------------------------------------------

#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn boto3_workflow_through_gateway() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let server = minio::MinioServer::start().expect("start minio");
    let gw = Gateway::spawn(&server.endpoint, &[]);
    gw.wait_ready();

    let python = workspace_root().join("tools/venv/bin/python");
    let python = if python.is_file() {
        python
    } else {
        PathBuf::from("python3")
    };
    let script = workspace_root().join("crates/sparta-testkit/clients/boto3_workflow.py");
    let size_mb = std::env::var("SPARTA_E2E_SIZE_MB").unwrap_or_else(|_| "256".into());

    let status = Command::new(python)
        .arg(script)
        .args(["--endpoint", &gw.endpoint, "--size-mb", &size_mb])
        .env("AWS_ACCESS_KEY_ID", minio::ACCESS_KEY)
        .env("AWS_SECRET_ACCESS_KEY", minio::SECRET_KEY)
        .env("AWS_DEFAULT_REGION", "us-east-1")
        .status()
        .expect("run boto3 workflow");
    assert!(status.success(), "boto3 workflow failed");

    // Shadow observation actually happened.
    let stats = gw.stats();
    let logical = stats["shadow"]["logical_bytes"].as_u64().unwrap();
    assert!(
        logical >= 256 * 1024 * 1024,
        "shadow tap saw only {logical} bytes"
    );
    assert!(
        stats["ops"]["op_put_object"].as_u64().unwrap_or(0) > 0
            || stats["ops"]["op_upload_part"].as_u64().unwrap_or(0) > 0
    );

    // The meter log must verify (tamper-evident chain) and cover the bytes.
    std::thread::sleep(Duration::from_millis(1500)); // allow a level record
    let verify = Command::new(meter_bin())
        .args(["--log", gw.meter_path.to_str().unwrap(), "verify"])
        .output()
        .expect("run sparta-meter verify");
    assert!(
        verify.status.success(),
        "meter chain verify failed: {}",
        String::from_utf8_lossy(&verify.stderr)
    );
    let text = std::fs::read_to_string(&gw.meter_path).unwrap();
    assert!(text.contains("\"op\":\"put_object\"") || text.contains("\"op\":\"upload_part\""));
    assert!(
        text.contains("\"kind\":\"level\""),
        "projection level records missing"
    );
}

#[test]
#[ignore = "needs MinIO and the aws CLI; run with --ignored"]
fn aws_cli_workflow_through_gateway() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let cli = workspace_root().join("tools/venv/bin/aws");
    if !cli.is_file() {
        eprintln!("SKIP: aws CLI not found at {}", cli.display());
        return;
    }
    let server = minio::MinioServer::start().expect("start minio");
    let gw = Gateway::spawn(&server.endpoint, &[]);
    gw.wait_ready();

    let script = workspace_root().join("crates/sparta-testkit/clients/aws_cli_workflow.sh");
    let status = Command::new("bash")
        .arg(script)
        .args([&gw.endpoint, "sparta-cli-e2e", "128"])
        .env("AWS_ACCESS_KEY_ID", minio::ACCESS_KEY)
        .env("AWS_SECRET_ACCESS_KEY", minio::SECRET_KEY)
        .env("AWS_DEFAULT_REGION", "us-east-1")
        .env("AWS_CLI_BIN", cli)
        .status()
        .expect("run aws cli workflow");
    assert!(status.success(), "aws cli workflow failed");
}

/// The full boto3 workflow with every fault class active at once: upstream
/// 500s on first attempts, a panicking chunker, and a dead metadata DB. The
/// unmodified client must still pass byte-identically (Invariant 0).
#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn boto3_workflow_under_faults() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = minio::MinioServer::start().expect("start minio");
    let proxy = rt.block_on(FaultProxy::start(&server.endpoint)).unwrap();
    let gw = Gateway::spawn(
        &proxy.endpoint,
        &[
            ("SPARTA_FAULT_PANIC_TAP", "1"),
            ("SPARTA_FAULT_DB_FAIL", "1"),
        ],
    );
    gw.wait_ready();
    proxy.set_fault(Fault::FirstAttempt500);

    let python = workspace_root().join("tools/venv/bin/python");
    let python = if python.is_file() {
        python
    } else {
        PathBuf::from("python3")
    };
    let script = workspace_root().join("crates/sparta-testkit/clients/boto3_workflow.py");
    let status = Command::new(python)
        .arg(script)
        .args([
            "--endpoint",
            &gw.endpoint,
            "--size-mb",
            "192",
            "--bucket",
            "faulted-wf",
        ])
        .env("AWS_ACCESS_KEY_ID", minio::ACCESS_KEY)
        .env("AWS_SECRET_ACCESS_KEY", minio::SECRET_KEY)
        .env("AWS_DEFAULT_REGION", "us-east-1")
        .status()
        .expect("run boto3 workflow under faults");
    assert!(
        status.success(),
        "boto3 workflow must pass with all faults active"
    );
    let (_, injected) = proxy.stats();
    assert!(injected > 0, "faults were not actually injected");
}

#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn fault_suite() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = minio::MinioServer::start().expect("start minio");
    let proxy = rt.block_on(FaultProxy::start(&server.endpoint)).unwrap();

    let gw = Gateway::spawn(&proxy.endpoint, &[]);
    gw.wait_ready();
    let client = rt.block_on(seed::client_for_endpoint(&gw.endpoint));
    rt.block_on(seed::create_bucket(&client, "faults")).unwrap();

    let body = test_payload(10, 0xFA);

    // Baseline.
    rt.block_on(roundtrip(&client, "faults", "base/obj.bin", &body));

    // Upstream 500 on first attempt of every request: the gateway's retry
    // layer must absorb it; clients see success with correct bytes.
    proxy.set_fault(Fault::FirstAttempt500);
    rt.block_on(roundtrip(&client, "faults", "f500/obj.bin", &body));
    rt.block_on(async {
        let listed = client
            .list_objects_v2()
            .bucket("faults")
            .send()
            .await
            .expect("list under 500 faults");
        assert!(listed.key_count().unwrap_or(0) >= 2);
        client
            .copy_object()
            .bucket("faults")
            .key("f500/copy.bin")
            .copy_source("faults/f500/obj.bin")
            .send()
            .await
            .expect("copy under 500 faults");
        client
            .delete_object()
            .bucket("faults")
            .key("f500/copy.bin")
            .send()
            .await
            .expect("delete under 500 faults");
    });
    let (_, injected) = proxy.stats();
    assert!(injected > 0, "no faults were actually injected");

    // Connection drop on first attempt.
    proxy.set_fault(Fault::FirstAttemptReset);
    rt.block_on(roundtrip(&client, "faults", "freset/obj.bin", &body));

    // First-attempt delay: forces slow-start; request must still complete.
    proxy.set_fault(Fault::FirstAttemptDelayMs(1500));
    rt.block_on(roundtrip(&client, "faults", "fdelay/obj.bin", &body));

    // Hard outage: the gateway forwards the failure (behavior parity with the
    // raw endpoint) and must recover instantly once upstream heals.
    proxy.set_fault(Fault::Always500);
    rt.block_on(async {
        let err = client
            .get_object()
            .bucket("faults")
            .key("base/obj.bin")
            .send()
            .await;
        assert!(err.is_err(), "hard outage must surface, not hang or lie");
    });
    proxy.set_fault(Fault::None);
    rt.block_on(roundtrip(&client, "faults", "recovered/obj.bin", &body));
}

#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn fault_dead_metadata_db() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = minio::MinioServer::start().expect("start minio");
    // Every DB call fails from the first request on.
    let gw = Gateway::spawn(&server.endpoint, &[("SPARTA_FAULT_DB_FAIL", "1")]);
    gw.wait_ready();
    let client = rt.block_on(seed::client_for_endpoint(&gw.endpoint));
    rt.block_on(seed::create_bucket(&client, "dbdead")).unwrap();
    let body = test_payload(8, 0xDB);
    rt.block_on(roundtrip(&client, "dbdead", "obj.bin", &body));
}

#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn fault_panicking_chunker() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = minio::MinioServer::start().expect("start minio");
    let gw = Gateway::spawn(&server.endpoint, &[("SPARTA_FAULT_PANIC_TAP", "1")]);
    gw.wait_ready();
    let client = rt.block_on(seed::client_for_endpoint(&gw.endpoint));
    rt.block_on(seed::create_bucket(&client, "panics")).unwrap();
    let body = test_payload(8, 0x9A);
    rt.block_on(roundtrip(&client, "panics", "obj.bin", &body));
}

#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn bypass_file_toggles_per_request() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = minio::MinioServer::start().expect("start minio");
    let bypass_dir = tempfile::tempdir().unwrap();
    let bypass_file = bypass_dir.path().join("bypass");
    let gw = Gateway::spawn(
        &server.endpoint,
        &[("SPARTA_BYPASS_FILE", bypass_file.to_str().unwrap())],
    );
    gw.wait_ready();
    let client = rt.block_on(seed::client_for_endpoint(&gw.endpoint));
    rt.block_on(seed::create_bucket(&client, "bypass")).unwrap();

    let body = test_payload(6, 0xBF);
    rt.block_on(roundtrip(&client, "bypass", "observed.bin", &body));
    let observed = gw.stats()["shadow"]["logical_bytes"].as_u64().unwrap();
    assert!(observed > 0, "shadow tap should observe before bypass");

    // Flip to dumb proxy with no restart.
    std::fs::write(&bypass_file, b"1").unwrap();
    rt.block_on(roundtrip(&client, "bypass", "bypassed.bin", &body));
    let after = gw.stats()["shadow"]["logical_bytes"].as_u64().unwrap();
    assert_eq!(
        observed, after,
        "bypassed requests must not be observed (dumb proxy)"
    );

    // Flip back.
    std::fs::remove_file(&bypass_file).unwrap();
    rt.block_on(roundtrip(&client, "bypass", "observed2.bin", &body));
    let last = gw.stats()["shadow"]["logical_bytes"].as_u64().unwrap();
    assert!(last > after, "tap must resume after bypass clears");
}

#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn readyz_tracks_upstream() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let server = minio::MinioServer::start().expect("start minio");
    let gw = Gateway::spawn(&server.endpoint, &[]);
    gw.wait_ready();
    assert_eq!(http_status(gw.port, "/healthz"), Some(200));
    assert_eq!(http_status(gw.port, "/readyz"), Some(200));

    // Kill upstream: /readyz must go 503 (LB fails over) while /healthz stays 200.
    drop(server);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if http_status(gw.port, "/readyz") == Some(503) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "/readyz never flipped to 503 after upstream death"
        );
        std::thread::sleep(Duration::from_millis(300));
    }
    assert_eq!(http_status(gw.port, "/healthz"), Some(200));
}

#[test]
#[ignore = "needs MinIO; run with --ignored"]
fn perf_sanity_passthrough_overhead() {
    if minio::find_minio().is_none() {
        eprintln!("SKIP: minio binary not found");
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = minio::MinioServer::start().expect("start minio");
    let gw = Gateway::spawn(&server.endpoint, &[]);
    gw.wait_ready();

    let direct = rt.block_on(seed::client_for_endpoint(&server.endpoint));
    let via_gw = rt.block_on(seed::client_for_endpoint(&gw.endpoint));
    rt.block_on(seed::create_bucket(&direct, "perf")).unwrap();

    let body = test_payload(1, 0x9F);
    let p50 = |mut xs: Vec<Duration>| {
        xs.sort();
        xs[xs.len() / 2]
    };
    let run = |client: aws_sdk_s3::Client, prefix: &'static str| {
        let body = body.clone();
        let mut times = Vec::new();
        rt.block_on(async {
            for i in 0..30 {
                let start = Instant::now();
                client
                    .put_object()
                    .bucket("perf")
                    .key(format!("{prefix}/{i}.bin"))
                    .body(aws_sdk_s3::primitives::ByteStream::from(body.clone()))
                    .send()
                    .await
                    .unwrap();
                times.push(start.elapsed());
            }
        });
        p50(times)
    };
    let direct_p50 = run(direct, "direct");
    let gw_p50 = run(via_gw, "gw");
    let overhead = gw_p50.saturating_sub(direct_p50);
    println!(
        "perf sanity: direct p50 {direct_p50:?}, gateway p50 {gw_p50:?}, added {overhead:?} (target < 5ms)"
    );
    // Sanity bound only (spec: target, not gate): catch order-of-magnitude
    // regressions without flaking CI.
    assert!(
        overhead < Duration::from_millis(50),
        "pathological passthrough overhead: {overhead:?}"
    );
}
