//! HTTP serving: health endpoints wrapped around the s3s service, plus the
//! upstream reachability probe backing `/readyz`.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as ConnBuilder;
use s3s::service::S3Service;
use s3s::{Body, HttpResponse};
use tokio::net::TcpListener;
use tracing::{info, warn};

/// Liveness/readiness state shared with the probe task.
pub struct Health {
    pub upstream_ok: AtomicBool,
}

/// `/healthz`: process is alive. `/readyz`: upstream reachable — returns 503
/// otherwise so a load balancer fails over to the raw endpoint (Invariant 0.3).
#[derive(Clone)]
pub struct GatewayHttp {
    pub s3: S3Service,
    pub health: Arc<Health>,
}

fn text_response(status: u16, body: &'static str) -> HttpResponse {
    http::Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .body(Body::from(body.to_string()))
        .expect("static response")
}

impl hyper::service::Service<http::Request<Incoming>> for GatewayHttp {
    type Response = HttpResponse;
    type Error = s3s::HttpError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: http::Request<Incoming>) -> Self::Future {
        match req.uri().path() {
            "/healthz" => Box::pin(async { Ok(text_response(200, "ok\n")) }),
            "/readyz" => {
                let ok = self.health.upstream_ok.load(Ordering::Relaxed);
                Box::pin(async move {
                    if ok {
                        Ok(text_response(200, "ready\n"))
                    } else {
                        Ok(text_response(503, "upstream unreachable\n"))
                    }
                })
            }
            _ => {
                let svc = self.s3.clone();
                Box::pin(async move { hyper::service::Service::call(&svc, req).await })
            }
        }
    }
}

/// Background task: TCP-probe the upstream endpoint and keep `/readyz` honest.
pub fn spawn_upstream_probe(endpoint: String, health: Arc<Health>) {
    tokio::spawn(async move {
        let target = match endpoint
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split('/')
            .next()
        {
            Some(host) if !host.is_empty() => {
                let has_port = host
                    .rsplit_once(':')
                    .is_some_and(|(_, p)| p.parse::<u16>().is_ok());
                if has_port {
                    host.to_string()
                } else if endpoint.starts_with("https://") {
                    format!("{host}:443")
                } else {
                    format!("{host}:80")
                }
            }
            _ => {
                warn!(
                    endpoint,
                    "cannot parse upstream endpoint for readiness probe"
                );
                return;
            }
        };
        loop {
            let ok = tokio::time::timeout(
                Duration::from_secs(3),
                tokio::net::TcpStream::connect(&target),
            )
            .await
            .map(|r| r.is_ok())
            .unwrap_or(false);
            let was = health.upstream_ok.swap(ok, Ordering::Relaxed);
            if was != ok {
                if ok {
                    info!(target, "upstream reachable; /readyz -> 200");
                } else {
                    warn!(
                        target,
                        "upstream unreachable; /readyz -> 503 (LB should fail over)"
                    );
                }
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
}

/// Accept loop.
pub async fn serve(listener: TcpListener, svc: GatewayHttp) -> anyhow::Result<()> {
    info!(addr = %listener.local_addr()?, "sparta-gateway listening");
    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let svc = svc.clone();
        tokio::spawn(async move {
            let result = ConnBuilder::new(TokioExecutor::new())
                .serve_connection(io, svc)
                .await;
            if let Err(e) = result {
                // Connection-level errors are normal (client disconnects).
                tracing::debug!(error = %e, "connection closed with error");
            }
        });
    }
}
