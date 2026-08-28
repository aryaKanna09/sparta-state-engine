//! Local MinIO server harness for integration tests.
//!
//! Finds the server binary via `SPARTA_TEST_MINIO`, `tools/bin/minio` at the
//! workspace root, or `minio` on PATH. Each harness instance runs an isolated
//! server on a random port with a temp data directory, killed on drop.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

pub const ACCESS_KEY: &str = "sparta-test";
pub const SECRET_KEY: &str = "sparta-test-secret";

pub struct MinioServer {
    child: Child,
    pub endpoint: String,
    pub port: u16,
    _data_dir: tempfile::TempDir,
}

/// Locate the MinIO binary, if any. Tests should skip (not fail) when absent.
pub fn find_minio() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SPARTA_TEST_MINIO") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    // Walk up from the crate dir to find the workspace's tools/bin/minio.
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..4 {
        let candidate = dir.join("tools/bin/minio");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    which("minio")
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

impl MinioServer {
    /// Start a fresh server; waits until the health endpoint answers.
    pub fn start() -> Result<Self> {
        let Some(binary) = find_minio() else {
            bail!("minio binary not found (set SPARTA_TEST_MINIO or place it in tools/bin)");
        };
        let data_dir = tempfile::tempdir().context("minio temp dir")?;
        let port = free_port();
        let child = Command::new(&binary)
            .arg("server")
            .arg(data_dir.path())
            .arg("--address")
            .arg(format!("127.0.0.1:{port}"))
            .env("MINIO_ROOT_USER", ACCESS_KEY)
            .env("MINIO_ROOT_PASSWORD", SECRET_KEY)
            .env("MINIO_BROWSER", "off")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn {}", binary.display()))?;
        let server = Self {
            child,
            endpoint: format!("http://127.0.0.1:{port}"),
            port,
            _data_dir: data_dir,
        };
        server.wait_ready(Duration::from_secs(30))?;
        Ok(server)
    }

    fn wait_ready(&self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if http_ok(self.port, "/minio/health/live") {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        bail!("minio did not become ready within {timeout:?}");
    }
}

impl Drop for MinioServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Minimal HTTP GET, just enough for the MinIO health endpoints.
fn http_ok(port: u16, path: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 64];
    match stream.read(&mut buf) {
        Ok(n) if n > 0 => String::from_utf8_lossy(&buf[..n]).contains(" 200 "),
        _ => false,
    }
}

/// Standard AWS env for pointing CLIs/SDKs at a harness server.
pub fn aws_env(server: &MinioServer) -> Vec<(&'static str, String)> {
    vec![
        ("AWS_ACCESS_KEY_ID", ACCESS_KEY.to_string()),
        ("AWS_SECRET_ACCESS_KEY", SECRET_KEY.to_string()),
        ("AWS_REGION", "us-east-1".to_string()),
        ("AWS_ENDPOINT_URL", server.endpoint.clone()),
    ]
}
