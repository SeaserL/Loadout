// SPDX-License-Identifier: Apache-2.0

use reqwest::Url;

use crate::ProviderError;

/// Whether the URL points at this machine.
pub fn is_loopback_url(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        return ip.is_loopback();
    }
    host.eq_ignore_ascii_case("localhost")
}

/// Validates a provider base URL (ExecPlan 002 §9): https, or http only for loopback unless
/// `allow_insecure` is set.
pub fn validate_base_url(raw: &str, allow_insecure: bool) -> Result<Url, ProviderError> {
    let url = Url::parse(raw.trim())
        .map_err(|e| ProviderError::Config(format!("invalid base URL {raw:?}: {e}")))?;
    match url.scheme() {
        "https" => {}
        "http" if is_loopback_url(&url) || allow_insecure => {}
        "http" => {
            return Err(ProviderError::Config(
                "plain http is only allowed for localhost; use https or pass --allow-insecure-http"
                    .into(),
            ));
        }
        other => {
            return Err(ProviderError::Config(format!(
                "unsupported URL scheme {other:?}"
            )));
        }
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(ProviderError::Config(
            "base URL must not contain a query or fragment".into(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ProviderError::Config(
            "base URL must not contain credentials".into(),
        ));
    }
    Ok(url)
}

/// `base` + `path` with exactly one slash between them.
pub fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_schemes_and_loopback() {
        assert!(validate_base_url("https://api.example.com/v1", false).is_ok());
        assert!(validate_base_url("http://127.0.0.1:11434/v1", false).is_ok());
        assert!(validate_base_url("http://localhost:8000/v1", false).is_ok());
        assert!(validate_base_url("http://[::1]:8000/v1", false).is_ok());
        assert!(validate_base_url("http://10.0.0.5/v1", false).is_err());
        assert!(validate_base_url("http://10.0.0.5/v1", true).is_ok());
        assert!(validate_base_url("ftp://x/v1", true).is_err());
        assert!(validate_base_url("https://user:pw@x/v1", false).is_err());
        assert!(validate_base_url("https://x/v1?key=1", false).is_err());
        assert!(validate_base_url("not a url", false).is_err());
    }

    #[test]
    fn joins_paths() {
        assert_eq!(
            join_url("https://x/v1/", "/chat/completions"),
            "https://x/v1/chat/completions"
        );
        assert_eq!(join_url("https://x/v1", "models"), "https://x/v1/models");
    }
}
