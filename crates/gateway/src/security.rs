// SPDX-License-Identifier: Apache-2.0

//! Host / Origin checks against DNS rebinding and browser cross-site calls, then client-key
//! authentication (ADR-011). Order: host → origin → auth.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::{HOST, ORIGIN};
use axum::middleware::Next;
use axum::response::Response;

use crate::{ApiError, ApiFlavor, AuthError, ClientKeyIndex, ErrorCategory};

#[derive(Debug)]
pub(crate) struct SecurityPolicy {
    allowed_hosts: Vec<String>,
    allowed_origins: Vec<String>,
    keys: Arc<ClientKeyIndex>,
}

impl SecurityPolicy {
    pub(crate) fn new(port: u16, allowed_origins: Vec<String>, keys: Arc<ClientKeyIndex>) -> Self {
        Self {
            allowed_hosts: vec![
                format!("127.0.0.1:{port}"),
                format!("localhost:{port}"),
                format!("[::1]:{port}"),
            ],
            allowed_origins: allowed_origins
                .into_iter()
                .map(|o| o.trim_end_matches('/').to_ascii_lowercase())
                .collect(),
            keys,
        }
    }

    fn host_allowed(&self, host: &str) -> bool {
        self.allowed_hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(host))
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        let origin = origin.trim_end_matches('/').to_ascii_lowercase();
        self.allowed_origins.contains(&origin)
    }
}

fn reject(req: &Request, category: ErrorCategory, message: &str) -> Response {
    ApiError::new(category, message).into_response_for(ApiFlavor::from_path(req.uri().path()))
}

pub(crate) async fn enforce(
    State(policy): State<Arc<SecurityPolicy>>,
    mut req: Request,
    next: Next,
) -> Response {
    // HTTP/1.1 carries the Host header; HTTP/2 carries :authority in the URI.
    let host = req
        .headers()
        .get(HOST)
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned)
        .or_else(|| req.uri().authority().map(|a| a.as_str().to_owned()));
    match host {
        Some(host) if policy.host_allowed(&host) => {}
        _ => {
            tracing::warn!(host = ?host, "rejected request with unexpected Host");
            return reject(
                &req,
                ErrorCategory::MisdirectedRequest,
                "unexpected Host header; Loadout only answers requests addressed to localhost",
            );
        }
    }

    if let Some(origin) = req.headers().get(ORIGIN) {
        let allowed = origin
            .to_str()
            .map(|o| policy.origin_allowed(o))
            .unwrap_or(false);
        if !allowed {
            tracing::warn!("rejected browser request from a non-allowlisted Origin");
            return reject(
                &req,
                ErrorCategory::ForbiddenOrigin,
                "browser origin not allowed; add it to gateway.allowed_origins",
            );
        }
    }

    match policy.keys.authenticate(req.headers()) {
        Ok(client) => {
            req.extensions_mut().insert(client);
            next.run(req).await
        }
        Err(e @ (AuthError::Missing | AuthError::Invalid)) => {
            reject(&req, ErrorCategory::Unauthorized, &e.to_string())
        }
    }
}
