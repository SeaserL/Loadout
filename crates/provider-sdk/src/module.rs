// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::pin::Pin;

use loadout_protocol::{Protocol, chat, messages};
use loadout_secrets::Secret;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;

use crate::usage::UsageReport;
use crate::{
    CatalogModel, HttpClient, ProviderDescriptor, ProviderError, TrustLevel, UpstreamFailure,
    classify,
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Provider setting that pins the usage source (`auto`, `off` or a module-listed source).
pub const USAGE_SOURCE_SETTING: &str = "usage_source";

/// A configured provider instance (a row of `providers`).
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderConfig {
    pub slug: String,
    pub kind: String,
    pub base_url: Option<String>,
    /// Module-specific settings (`providers.config_json`).
    pub settings: Value,
}

impl ProviderConfig {
    pub fn setting_str(&self, key: &str) -> Option<&str> {
        self.settings.get(key).and_then(Value::as_str)
    }

    pub fn setting_bool(&self, key: &str) -> Option<bool> {
        self.settings.get(key).and_then(Value::as_bool)
    }

    /// The pinned usage source (`usage_source` setting): `auto` unless set.
    pub fn usage_source(&self) -> &str {
        self.setting_str(USAGE_SOURCE_SETTING).unwrap_or("auto")
    }

    /// The configured base URL, or the module default.
    pub fn base_url(&self, descriptor: &ProviderDescriptor) -> Result<String, ProviderError> {
        self.base_url
            .clone()
            .or_else(|| descriptor.default_base_url.map(str::to_owned))
            .ok_or_else(|| ProviderError::Config(format!("provider {} has no base URL", self.slug)))
    }
}

/// Where and how to send an upstream request. Header values carrying credentials are marked
/// sensitive, so they never appear in `Debug` output.
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub url: String,
    pub headers: HeaderMap,
}

impl Endpoint {
    pub fn new(url: String) -> Self {
        Self {
            url,
            headers: HeaderMap::new(),
        }
    }

    pub fn header(mut self, name: &'static str, value: &str) -> Result<Self, ProviderError> {
        let v = HeaderValue::from_str(value)
            .map_err(|_| ProviderError::Config(format!("invalid value for header {name}")))?;
        self.headers.insert(HeaderName::from_static(name), v);
        Ok(self)
    }

    pub fn secret_header(
        mut self,
        name: &'static str,
        prefix: &str,
        secret: &Secret,
    ) -> Result<Self, ProviderError> {
        let mut v = HeaderValue::from_str(&format!("{prefix}{}", secret.expose()))
            .map_err(|_| ProviderError::Config("API key contains invalid characters".into()))?;
        v.set_sensitive(true);
        self.headers.insert(HeaderName::from_static(name), v);
        Ok(self)
    }
}

/// Everything a driver needs to talk to its upstream.
#[derive(Clone, Copy, Debug)]
pub struct DriverContext<'a> {
    pub http: &'a HttpClient,
    pub config: &'a ProviderConfig,
    pub key: Option<&'a Secret>,
    /// The user's home directory, where official CLIs keep their state (discovery and
    /// external-managed usage). Overridable for tests.
    pub home: Option<&'a std::path::Path>,
}

/// A Provider module (ADR-005). Only [`ProviderModule::descriptor`] is mandatory; every
/// driver method has a default meaning "not supported".
pub trait ProviderModule: Send + Sync + 'static {
    fn descriptor(&self) -> &'static ProviderDescriptor;

    /// Validates user-supplied configuration when the provider is added.
    fn validate(&self, config: &ProviderConfig) -> Result<(), ProviderError> {
        config.base_url(self.descriptor()).map(|_| ())
    }

    /// Trust level of the transport for `protocol` with this configuration.
    fn trust(&self, _config: &ProviderConfig, protocol: Protocol) -> Option<TrustLevel> {
        self.descriptor().transport(protocol).map(|t| t.trust)
    }

    // ---- transport driver ----

    fn endpoint(
        &self,
        _config: &ProviderConfig,
        _protocol: Protocol,
        _key: Option<&Secret>,
    ) -> Result<Endpoint, ProviderError> {
        Err(ProviderError::NotSupported("inference"))
    }

    fn chat_options(&self, _config: &ProviderConfig) -> chat::EncodeOptions {
        chat::EncodeOptions::default()
    }

    fn messages_options(&self, _config: &ProviderConfig) -> messages::EncodeOptions {
        messages::EncodeOptions::default()
    }

    /// Last change to an upstream request body before it is sent, for both passthrough and
    /// converted requests (ExecPlan 010 §2.1). Runs on the request path: no I/O.
    fn prepare_request(&self, _config: &ProviderConfig, _protocol: Protocol, _body: &mut Value) {}

    /// Upstream header that carries a stable per-conversation id (ADR-013), e.g.
    /// `x-opencode-session`. The gateway derives the value; `None` sends nothing.
    fn session_header(&self) -> Option<HeaderName> {
        None
    }

    /// The upstream only answers streaming requests; non-streaming clients get a response
    /// aggregated from the stream.
    fn upstream_stream_only(&self) -> bool {
        false
    }

    /// Concurrent requests allowed per account (`None`: unlimited).
    fn account_concurrency(&self) -> Option<u32> {
        None
    }

    // ---- auth driver ----

    /// Loadout-managed OAuth login (`AuthMethod::OAuth` modules).
    fn oauth(&self) -> Option<&dyn crate::oauth::OAuthFlow> {
        None
    }

    /// Maps an upstream HTTP error; override to recognize provider-specific quota errors.
    fn classify(&self, status: u16, headers: &HeaderMap, body: &str) -> UpstreamFailure {
        classify(status, headers, body)
    }

    // ---- discovery driver ----

    /// Inspects the local machine (Harness §12). Only called when the user runs a scan.
    fn discover(&self, _home: &std::path::Path) -> Option<crate::Discovered> {
        None
    }

    // ---- catalog driver ----

    fn list_models<'a>(
        &'a self,
        _ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<Vec<CatalogModel>, ProviderError>> {
        Box::pin(async { Err(ProviderError::NotSupported("model discovery")) })
    }

    // ---- usage driver ----

    /// Usage sources the user may pin with the `usage_source` setting, besides the always
    /// available `auto` and `off` (ExecPlan 008), e.g. `["endpoint", "local"]`.
    fn usage_sources(&self) -> &'static [&'static str] {
        &[]
    }

    /// How often the background poller refreshes usage (ExecPlan 003 §4.2).
    fn usage_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(300)
    }

    fn fetch_usage<'a>(
        &'a self,
        _ctx: DriverContext<'a>,
    ) -> BoxFuture<'a, Result<UsageReport, ProviderError>> {
        Box::pin(async { Err(ProviderError::NotSupported("usage")) })
    }

    /// Quota state an upstream reports on a successful inference response (passive usage,
    /// ExecPlan 013). Called by the gateway after route selection, once per response; must be
    /// cheap and must not block. The default reports nothing.
    fn usage_from_headers(&self, _headers: &HeaderMap) -> Option<UsageReport> {
        None
    }
}
