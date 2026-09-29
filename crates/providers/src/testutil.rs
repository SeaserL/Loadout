// SPDX-License-Identifier: Apache-2.0

//! Minimal mock upstream for driver tests.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use loadout_provider_sdk::{HttpClient, HttpConfig, ProviderConfig};
use serde_json::Value;

pub(crate) fn config(slug: &str, kind: &str, base_url: &str) -> ProviderConfig {
    ProviderConfig {
        slug: slug.into(),
        kind: kind.into(),
        base_url: Some(base_url.into()),
        settings: Value::Object(Default::default()),
    }
}

pub(crate) fn http() -> HttpClient {
    HttpClient::new(&HttpConfig::default()).expect("http client")
}

pub(crate) struct MockUpstream {
    pub base: String,
    headers: Arc<Mutex<Option<HeaderMap>>>,
}

impl MockUpstream {
    pub async fn json(path: &str, body: Value) -> Self {
        Self::respond(path, 200, body.to_string()).await
    }

    pub async fn status(path: &str, status: u16, body: &str) -> Self {
        Self::respond(path, status, body.to_owned()).await
    }

    async fn respond(path: &str, status: u16, body: String) -> Self {
        let headers: Arc<Mutex<Option<HeaderMap>>> = Arc::default();
        let seen = headers.clone();
        let app = Router::new().route(
            path,
            get(move |h: HeaderMap| {
                let body = body.clone();
                let seen = seen.clone();
                async move {
                    *seen.lock().expect("lock") = Some(h);
                    (
                        StatusCode::from_u16(status).expect("status"),
                        [("content-type", "application/json")],
                        body,
                    )
                        .into_response()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            base: format!("http://{addr}"),
            headers,
        }
    }

    pub fn last_header(&self, name: &str) -> Option<String> {
        self.headers
            .lock()
            .expect("lock")
            .as_ref()?
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    }
}
