//! Shared harness for gateway integration tests.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use sparta_testkit::minio;

pub fn gateway_bin() -> &'static str {
    env!("CARGO_BIN_EXE_sparta-gateway")
}

pub fn workspace_root() -> PathBuf {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    d.pop();
    d.pop();
    d
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub fn http_status(port: u16, path: &str) -> Option<u16> {
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

pub struct Gateway {
    child: Child,
    pub port: u16,
    pub endpoint: String,
    pub tmp: tempfile::TempDir,
    pub db_path: PathBuf,
    pub meter_path: PathBuf,
}

impl Gateway {
    pub fn spawn(upstream: &str, extra_env: &[(&str, &str)]) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        Self::spawn_with_dirs(upstream, extra_env, tmp)
    }

    /// Spawn using an existing temp dir (lets tests restart the gateway with
    /// or without its previous DB).
    pub fn spawn_with_dirs(
        upstream: &str,
        extra_env: &[(&str, &str)],
        tmp: tempfile::TempDir,
    ) -> Self {
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
            tmp,
            db_path,
            meter_path,
        };
        gw.wait_healthy();
        gw
    }

    pub fn wait_healthy(&self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if http_status(self.port, "/healthz") == Some(200) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("gateway did not become healthy");
    }

    pub fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if http_status(self.port, "/readyz") == Some(200) {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("gateway did not become ready (upstream probe)");
    }

    pub fn stats(&self) -> serde_json::Value {
        let out = Command::new(gateway_bin())
            .arg("stats")
            .env("SPARTA_DB_PATH", &self.db_path)
            .env("SPARTA_UPSTREAM_ENDPOINT", "http://unused:1")
            .output()
            .expect("gateway stats");
        assert!(out.status.success(), "stats failed: {out:?}");
        serde_json::from_slice(&out.stdout).expect("stats JSON")
    }

    /// Kill the gateway and hand back the temp dir for a restart.
    pub fn shutdown(mut self) -> tempfile::TempDir {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Move the tempdir out without running Drop's kill twice.
        std::mem::replace(&mut self.tmp, tempfile::tempdir().unwrap())
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn test_payload(mb: usize, seed_v: u64) -> Vec<u8> {
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

/// Run a CLI subcommand of the gateway binary with the given env.
pub fn gateway_cli(args: &[&str], envs: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(gateway_bin());
    cmd.args(args).env("RUST_LOG", "warn");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("run gateway CLI")
}
