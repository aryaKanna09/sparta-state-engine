//! Fault-injecting HTTP proxy for the Invariant 0 suite.
//!
//! Sits between the gateway and MinIO and injects upstream failures on
//! command: HTTP 500s, connection resets, and first-attempt delays. Faults are
//! typically "first attempt per request key" so the gateway's retry policy is
//! what the tests exercise — exactly the behavior a flaky real upstream
//! produces. The proxy preserves the Host header so SigV4 signatures stay
//! valid end to end.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};

/// What to inject. `FirstAttempt*` variants fault only the first time a given
/// (method, path, query) is seen, so retries succeed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    None,
    /// 500 on every request (hard outage).
    Always500,
    /// 500 on the first attempt of each distinct request.
    FirstAttempt500,
    /// Drop the connection on the first attempt of each distinct request.
    FirstAttemptReset,
    /// Delay the first attempt of each distinct request by this many ms
    /// (drives client/attempt timeouts).
    FirstAttemptDelayMs(u64),
}

#[derive(Default)]
struct State {
    fault: Option<Fault>,
    seen: HashSet<String>,
    hits: u64,
    faults_injected: u64,
}

/// Handle to a running fault proxy.
pub struct FaultProxy {
    pub endpoint: String,
    state: Arc<Mutex<State>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl FaultProxy {
    /// Start a proxy forwarding to `target` (e.g. the MinIO endpoint).
    pub async fn start(target: &str) -> Result<Self> {
        let target = target.trim_end_matches('/').to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind fault proxy")?;
        let addr: SocketAddr = listener.local_addr()?;
        let state = Arc::new(Mutex::new(State {
            fault: Some(Fault::None),
            ..Default::default()
        }));
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();

        let st = Arc::clone(&state);
        tokio::spawn(async move {
            let client: Client<_, Incoming> = Client::builder(TokioExecutor::new()).build_http();
            loop {
                tokio::select! {
                    _ = &mut rx => break,
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break };
                        let io = TokioIo::new(stream);
                        let st = Arc::clone(&st);
                        let client = client.clone();
                        let target = target.clone();
                        tokio::spawn(async move {
                            let svc = service_fn(move |req: Request<Incoming>| {
                                handle(req, Arc::clone(&st), client.clone(), target.clone())
                            });
                            let _ = hyper::server::conn::http1::Builder::new()
                                .serve_connection(io, svc)
                                .await;
                        });
                    }
                }
            }
        });

        Ok(Self {
            endpoint: format!("http://{addr}"),
            state,
            shutdown: Some(tx),
        })
    }

    pub fn set_fault(&self, fault: Fault) {
        let mut s = self.state.lock().unwrap();
        s.fault = Some(fault);
        s.seen.clear();
    }

    pub fn stats(&self) -> (u64, u64) {
        let s = self.state.lock().unwrap();
        (s.hits, s.faults_injected)
    }
}

impl Drop for FaultProxy {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

enum Decision {
    Forward,
    Respond500,
    Reset,
    Delay(u64),
}

fn decide(state: &Arc<Mutex<State>>, key: &str) -> Decision {
    let mut s = state.lock().unwrap();
    s.hits += 1;
    let fault = s.fault.unwrap_or(Fault::None);
    let first = s.seen.insert(key.to_string());
    let inject = match fault {
        Fault::None => None,
        Fault::Always500 => Some(Decision::Respond500),
        Fault::FirstAttempt500 => first.then_some(Decision::Respond500),
        Fault::FirstAttemptReset => first.then_some(Decision::Reset),
        Fault::FirstAttemptDelayMs(ms) => first.then_some(Decision::Delay(ms)),
    };
    match inject {
        Some(d) => {
            s.faults_injected += 1;
            d
        }
        None => Decision::Forward,
    }
}

type ProxyBody = http_body_util::combinators::BoxBody<Bytes, hyper::Error>;

async fn handle(
    req: Request<Incoming>,
    state: Arc<Mutex<State>>,
    client: Client<hyper_util::client::legacy::connect::HttpConnector, Incoming>,
    target: String,
) -> Result<Response<ProxyBody>, hyper::Error> {
    let key = format!(
        "{} {} {}",
        req.method(),
        req.uri().path(),
        req.uri().query().unwrap_or("")
    );
    match decide(&state, &key) {
        Decision::Forward => {}
        Decision::Respond500 => {
            let body = Full::new(Bytes::from_static(
                b"<Error><Code>InternalError</Code><Message>injected fault</Message></Error>",
            ))
            .map_err(|e| match e {})
            .boxed();
            return Ok(Response::builder()
                .status(500)
                .header("content-type", "application/xml")
                .body(body)
                .unwrap());
        }
        Decision::Reset => {
            // Returning an Err tears down the connection abruptly.
            // hyper::Error can't be constructed; emulate a reset by panicking
            // inside this connection task after hijacking is not possible, so
            // instead return a malformed response the client treats as a
            // protocol error: close via empty 500 with connection: close.
            let body = Full::new(Bytes::new()).map_err(|e| match e {}).boxed();
            return Ok(Response::builder()
                .status(500)
                .header("connection", "close")
                .body(body)
                .unwrap());
        }
        Decision::Delay(ms) => {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
    }

    // Forward: rewrite the URI authority to the target, preserve everything
    // else (including Host, which the SigV4 signature covers).
    let (mut parts, body) = req.into_parts();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    parts.uri = format!("{target}{path_and_query}")
        .parse()
        .expect("valid forwarded uri");
    let forwarded = Request::from_parts(parts, body);
    let resp = match client.request(forwarded).await {
        Ok(r) => r.map(|b| b.boxed()),
        Err(e) => {
            let body = Full::new(Bytes::from(format!(
                "<Error><Code>BadGateway</Code><Message>{e}</Message></Error>"
            )))
            .map_err(|e| match e {})
            .boxed();
            Response::builder().status(502).body(body).unwrap()
        }
    };
    Ok(resp)
}
