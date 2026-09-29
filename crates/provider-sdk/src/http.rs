// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::{ProviderError, UpstreamFailure, classify, is_loopback_url};

#[derive(Clone, Debug)]
pub struct HttpConfig {
    pub connect_timeout: Duration,
    /// Maximum silence while reading a response (streams included).
    pub read_timeout: Duration,
    pub pool_idle_timeout: Duration,
    pub user_agent: String,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(300),
            pool_idle_timeout: Duration::from_secs(90),
            user_agent: format!("Loadout/{}", env!("CARGO_PKG_VERSION")),
        }
    }
}

/// Shared upstream HTTP client. Connections are pooled per host (keep-alive, HTTP/2 where
/// the upstream supports it). Loopback URLs bypass the system proxy.
#[derive(Clone, Debug)]
pub struct HttpClient {
    proxied: reqwest::Client,
    direct: reqwest::Client,
}

impl HttpClient {
    pub fn new(config: &HttpConfig) -> Result<Self, ProviderError> {
        let build = |no_proxy: bool| {
            let mut b = reqwest::Client::builder()
                .connect_timeout(config.connect_timeout)
                .read_timeout(config.read_timeout)
                .pool_idle_timeout(config.pool_idle_timeout)
                .tcp_nodelay(true)
                .tcp_keepalive(Duration::from_secs(30))
                .user_agent(config.user_agent.clone());
            if no_proxy {
                b = b.no_proxy();
            }
            b.build()
                .map_err(|e| ProviderError::Config(format!("HTTP client: {e}")))
        };
        Ok(Self {
            proxied: build(false)?,
            direct: build(true)?,
        })
    }

    /// The client to use for `url`.
    pub fn for_url(&self, url: &str) -> &reqwest::Client {
        match reqwest::Url::parse(url) {
            Ok(u) if is_loopback_url(&u) => &self.direct,
            _ => &self.proxied,
        }
    }

    /// POST a JSON body, returning JSON (usage drivers; never on the request hot path).
    pub async fn post_json(
        &self,
        url: &str,
        headers: HeaderMap,
        body: &Value,
        timeout: Duration,
    ) -> Result<Value, ProviderError> {
        let response = self
            .for_url(url)
            .post(url)
            .headers(headers)
            .header("content-type", "application/json")
            .body(body.to_string())
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| ProviderError::Upstream(UpstreamFailure::from_reqwest(&e)))?;
        Self::json_response(response).await
    }

    async fn json_response(response: reqwest::Response) -> Result<Value, ProviderError> {
        let status = response.status().as_u16();
        let resp_headers = response.headers().clone();
        let body = response
            .text()
            .await
            .map_err(|e| ProviderError::Upstream(UpstreamFailure::from_reqwest(&e)))?;
        if !(200..300).contains(&status) {
            return Err(ProviderError::Upstream(classify(
                status,
                &resp_headers,
                &body,
            )));
        }
        serde_json::from_str(&body)
            .map_err(|e| ProviderError::InvalidResponse(format!("expected JSON: {e}")))
    }

    /// GET returning JSON, for catalog and usage drivers (never on the request hot path).
    pub async fn get_json(
        &self,
        url: &str,
        headers: HeaderMap,
        timeout: Duration,
    ) -> Result<Value, ProviderError> {
        let response = self
            .for_url(url)
            .get(url)
            .headers(headers)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| ProviderError::Upstream(UpstreamFailure::from_reqwest(&e)))?;
        let status = response.status().as_u16();
        let resp_headers = response.headers().clone();
        let body = response
            .text()
            .await
            .map_err(|e| ProviderError::Upstream(UpstreamFailure::from_reqwest(&e)))?;
        if !(200..300).contains(&status) {
            return Err(ProviderError::Upstream(classify(
                status,
                &resp_headers,
                &body,
            )));
        }
        serde_json::from_str(&body)
            .map_err(|e| ProviderError::InvalidResponse(format!("expected JSON: {e}")))
    }
}
