// SPDX-License-Identifier: Apache-2.0

//! Errors rendered in the caller's protocol: OpenAI-style for `/v1/chat/completions`,
//! `/v1/responses` and everything else; Anthropic-style for `/v1/messages`.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Which error body shape a client expects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiFlavor {
    OpenAi,
    Anthropic,
}

impl ApiFlavor {
    pub fn from_path(path: &str) -> Self {
        if path == "/v1/messages" || path.starts_with("/v1/messages/") {
            Self::Anthropic
        } else {
            Self::OpenAi
        }
    }
}

/// Loadout error categories (glossary §8) plus gateway-local ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCategory {
    InvalidRequest,
    Unauthorized,
    ForbiddenOrigin,
    MisdirectedRequest,
    NotFound,
    NotImplemented,
    PayloadTooLarge,
    Auth,
    RateLimit,
    Quota,
    UpstreamTimeout,
    Upstream5xx,
    UnsupportedCapability,
    ModelNotFound,
    TransportUnavailable,
    NoEligibleTarget,
    /// The client key's policy does not allow the model (ExecPlan 006).
    ModelNotAllowed,
    /// The client key's own rate / daily limit (ExecPlan 006).
    ClientLimit,
    Internal,
}

impl ErrorCategory {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::Unauthorized => "invalid_api_key",
            Self::ForbiddenOrigin => "forbidden_origin",
            Self::MisdirectedRequest => "misdirected_request",
            Self::NotFound => "not_found",
            Self::NotImplemented => "not_implemented",
            Self::PayloadTooLarge => "payload_too_large",
            Self::Auth => "upstream_auth",
            Self::RateLimit => "rate_limit",
            Self::Quota => "quota_exhausted",
            Self::UpstreamTimeout => "upstream_timeout",
            Self::Upstream5xx => "upstream_error",
            Self::UnsupportedCapability => "unsupported_capability",
            Self::ModelNotFound => "model_not_found",
            Self::TransportUnavailable => "transport_unavailable",
            Self::NoEligibleTarget => "no_eligible_target",
            Self::ModelNotAllowed => "model_not_allowed",
            Self::ClientLimit => "client_limit",
            Self::Internal => "internal_error",
        }
    }

    pub fn status(self) -> StatusCode {
        match self {
            Self::InvalidRequest | Self::UnsupportedCapability => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::ForbiddenOrigin | Self::ModelNotAllowed => StatusCode::FORBIDDEN,
            Self::MisdirectedRequest => StatusCode::MISDIRECTED_REQUEST,
            Self::NotFound | Self::ModelNotFound => StatusCode::NOT_FOUND,
            Self::NotImplemented => StatusCode::NOT_IMPLEMENTED,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::RateLimit | Self::Quota | Self::ClientLimit => StatusCode::TOO_MANY_REQUESTS,
            Self::UpstreamTimeout => StatusCode::GATEWAY_TIMEOUT,
            Self::Auth | Self::Upstream5xx => StatusCode::BAD_GATEWAY,
            Self::TransportUnavailable | Self::NoEligibleTarget => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// OpenAI `error.type`.
    fn openai_type(self) -> &'static str {
        match self.status().as_u16() {
            400 | 404 | 413 | 421 => "invalid_request_error",
            401 => "authentication_error",
            403 => "permission_error",
            429 => "rate_limit_error",
            _ => "api_error",
        }
    }

    /// Anthropic `error.type`.
    fn anthropic_type(self) -> &'static str {
        match self.status().as_u16() {
            400 | 421 => "invalid_request_error",
            401 => "authentication_error",
            403 => "permission_error",
            404 => "not_found_error",
            413 => "request_too_large",
            429 => "rate_limit_error",
            503 => "overloaded_error",
            _ => "api_error",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ApiError {
    pub category: ErrorCategory,
    pub message: String,
}

impl ApiError {
    pub fn new(category: ErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }

    pub fn into_response_for(self, flavor: ApiFlavor) -> Response {
        let status = self.category.status();
        let body = match flavor {
            ApiFlavor::OpenAi => json!({
                "error": {
                    "message": self.message,
                    "type": self.category.openai_type(),
                    "param": null,
                    "code": self.category.code(),
                }
            }),
            ApiFlavor::Anthropic => json!({
                "type": "error",
                "error": {
                    "type": self.category.anthropic_type(),
                    "message": self.message,
                }
            }),
        };
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flavor_follows_path() {
        assert_eq!(ApiFlavor::from_path("/v1/messages"), ApiFlavor::Anthropic);
        assert_eq!(
            ApiFlavor::from_path("/v1/messages/count_tokens"),
            ApiFlavor::Anthropic
        );
        assert_eq!(
            ApiFlavor::from_path("/v1/chat/completions"),
            ApiFlavor::OpenAi
        );
        assert_eq!(ApiFlavor::from_path("/v1/responses"), ApiFlavor::OpenAi);
        assert_eq!(ApiFlavor::from_path("/anything"), ApiFlavor::OpenAi);
    }

    #[test]
    fn statuses_match_categories() {
        assert_eq!(ErrorCategory::Unauthorized.status(), 401);
        assert_eq!(ErrorCategory::MisdirectedRequest.status(), 421);
        assert_eq!(ErrorCategory::NotImplemented.status(), 501);
        assert_eq!(ErrorCategory::NoEligibleTarget.status(), 503);
    }
}
