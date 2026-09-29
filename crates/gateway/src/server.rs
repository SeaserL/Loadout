// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{get, post};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

use crate::ClientKeyIndex;
use crate::routes;
use crate::security::{self, SecurityPolicy};

pub const DEFAULT_PORT: u16 = 8317;

#[derive(Clone, Debug)]
pub struct GatewayConfig {
    /// Must be a loopback address (ADR-011).
    pub bind: SocketAddr,
    /// Browser origins allowed to call the gateway; empty by default.
    pub allowed_origins: Vec<String>,
    pub max_body_bytes: usize,
    /// How long shutdown waits for in-flight requests.
    pub shutdown_grace: Duration,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)),
            allowed_origins: Vec::new(),
            max_body_bytes: 32 * 1024 * 1024,
            shutdown_grace: Duration::from_secs(30),
        }
    }
}

/// A bound, not yet serving, gateway. Binding separately from serving lets the daemon fail
/// fast with a clear error when the port is taken.
#[derive(Debug)]
pub struct Gateway {
    listener: TcpListener,
    router: Router,
    addr: SocketAddr,
    grace: Duration,
}

impl Gateway {
    pub async fn bind(config: GatewayConfig, keys: Arc<ClientKeyIndex>) -> io::Result<Self> {
        if !config.bind.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the gateway may only listen on a loopback address",
            ));
        }
        let listener = TcpListener::bind(config.bind).await?;
        let addr = listener.local_addr()?;
        let router = build_router(addr.port(), &config, keys);
        Ok(Self {
            listener,
            router,
            addr,
            grace: config.shutdown_grace,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Serves until `shutdown` fires, then stops accepting and waits up to the grace period
    /// for in-flight requests.
    pub async fn serve(self, shutdown: CancellationToken) -> io::Result<()> {
        tracing::info!(addr = %self.addr, "gateway listening");
        let graceful = axum::serve(self.listener, self.router)
            .with_graceful_shutdown(shutdown.clone().cancelled_owned());
        let deadline = async {
            shutdown.cancelled().await;
            tokio::time::sleep(self.grace).await;
        };
        tokio::select! {
            result = graceful => result,
            () = deadline => {
                tracing::warn!("gateway shutdown grace period elapsed; dropping in-flight requests");
                Ok(())
            }
        }
    }
}

pub(crate) fn build_router(port: u16, config: &GatewayConfig, keys: Arc<ClientKeyIndex>) -> Router {
    let policy = Arc::new(SecurityPolicy::new(
        port,
        config.allowed_origins.clone(),
        keys,
    ));
    Router::new()
        .route("/v1/models", get(routes::list_models))
        .route("/v1/chat/completions", post(routes::not_implemented))
        .route("/v1/responses", post(routes::not_implemented))
        .route("/v1/messages", post(routes::not_implemented))
        .fallback(routes::not_found)
        .layer(DefaultBodyLimit::max(config.max_body_bytes))
        .layer(middleware::from_fn_with_state(policy, security::enforce))
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;
    use crate::{ClientId, generate_client_key};

    const PORT: u16 = 8317;

    struct Fixture {
        router: Router,
        key: String,
    }

    fn fixture(allowed_origins: Vec<String>) -> Fixture {
        let key = generate_client_key();
        let keys = Arc::new(ClientKeyIndex::new([(key.hash, ClientId::from("test"))]));
        let config = GatewayConfig {
            allowed_origins,
            ..GatewayConfig::default()
        };
        Fixture {
            router: build_router(PORT, &config, keys),
            key: key.key,
        }
    }

    fn request(method: &str, path: &str) -> axum::http::request::Builder {
        Request::builder()
            .method(method)
            .uri(path)
            .header("host", format!("127.0.0.1:{PORT}"))
    }

    async fn send(f: &Fixture, req: Request<Body>) -> (StatusCode, Value, axum::http::HeaderMap) {
        let resp = f.router.clone().oneshot(req).await.expect("response");
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.expect("body");
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body, headers)
    }

    #[tokio::test]
    async fn authenticated_models_request_succeeds() {
        let f = fixture(vec![]);
        let req = request("GET", "/v1/models")
            .header("authorization", format!("Bearer {}", f.key))
            .body(Body::empty())
            .expect("req");
        let (status, body, headers) = send(&f, req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({"object": "list", "data": []}));
        assert!(headers.contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn host_header_is_enforced() {
        let f = fixture(vec![]);
        for host in ["evil.example:8317", "127.0.0.1:9999", "127.0.0.1"] {
            let req = Request::builder()
                .uri("/v1/models")
                .header("host", host)
                .header("x-api-key", &f.key)
                .body(Body::empty())
                .expect("req");
            let (status, body, _) = send(&f, req).await;
            assert_eq!(status, StatusCode::MISDIRECTED_REQUEST, "host {host}");
            assert_eq!(body["error"]["code"], "misdirected_request");
        }
        for host in ["localhost:8317", "LOCALHOST:8317", "[::1]:8317"] {
            let req = Request::builder()
                .uri("/v1/models")
                .header("host", host)
                .header("x-api-key", &f.key)
                .body(Body::empty())
                .expect("req");
            assert_eq!(send(&f, req).await.0, StatusCode::OK, "host {host}");
        }
    }

    #[tokio::test]
    async fn browser_origins_are_rejected_unless_allowlisted() {
        let f = fixture(vec!["http://localhost:3000/".into()]);
        let blocked = request("GET", "/v1/models")
            .header("origin", "https://evil.example")
            .header("x-api-key", &f.key)
            .body(Body::empty())
            .expect("req");
        let (status, body, _) = send(&f, blocked).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "forbidden_origin");

        let allowed = request("GET", "/v1/models")
            .header("origin", "http://localhost:3000")
            .header("x-api-key", &f.key)
            .body(Body::empty())
            .expect("req");
        assert_eq!(send(&f, allowed).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_or_wrong_key_is_unauthorized_even_for_unknown_paths() {
        let f = fixture(vec![]);
        for path in ["/v1/models", "/does/not/exist"] {
            let req = request("GET", path).body(Body::empty()).expect("req");
            let (status, body, _) = send(&f, req).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
            assert_eq!(body["error"]["type"], "authentication_error");
        }
        let wrong = request("GET", "/v1/models")
            .header(
                "authorization",
                format!("Bearer {}", generate_client_key().key),
            )
            .body(Body::empty())
            .expect("req");
        assert_eq!(send(&f, wrong).await.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn errors_use_the_callers_protocol() {
        let f = fixture(vec![]);

        let anthropic = request("POST", "/v1/messages")
            .header("x-api-key", &f.key)
            .body(Body::from("{}"))
            .expect("req");
        let (status, body, _) = send(&f, anthropic).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["type"], "error");
        assert_eq!(body["error"]["type"], "api_error");

        let unauth_anthropic = request("POST", "/v1/messages")
            .body(Body::from("{}"))
            .expect("req");
        let (_, body, _) = send(&f, unauth_anthropic).await;
        assert_eq!(body["type"], "error");
        assert_eq!(body["error"]["type"], "authentication_error");

        for path in ["/v1/chat/completions", "/v1/responses"] {
            let openai = request("POST", path)
                .header("authorization", format!("Bearer {}", f.key))
                .body(Body::from("{}"))
                .expect("req");
            let (status, body, _) = send(&f, openai).await;
            assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{path}");
            assert_eq!(body["error"]["code"], "not_implemented");
        }

        let unknown = request("GET", "/v2/whatever")
            .header("x-api-key", &f.key)
            .body(Body::empty())
            .expect("req");
        assert_eq!(send(&f, unknown).await.0, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn non_loopback_bind_is_refused() {
        let config = GatewayConfig {
            bind: SocketAddr::from(([0, 0, 0, 0], 0)),
            ..GatewayConfig::default()
        };
        let err = Gateway::bind(config, Arc::default())
            .await
            .expect_err("must refuse");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
