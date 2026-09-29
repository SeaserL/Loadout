// SPDX-License-Identifier: Apache-2.0

//! Phase 1 skeleton endpoints. Phase 2 replaces the 501 handlers with real proxying.

use axum::Json;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::{ApiError, ApiFlavor, ErrorCategory};

pub(crate) async fn list_models() -> Response {
    Json(json!({ "object": "list", "data": [] })).into_response()
}

pub(crate) async fn not_implemented(req: Request) -> Response {
    let path = req.uri().path();
    ApiError::new(
        ErrorCategory::NotImplemented,
        format!("{path} is not available yet in this Loadout build"),
    )
    .into_response_for(ApiFlavor::from_path(path))
}

pub(crate) async fn not_found(req: Request) -> Response {
    let path = req.uri().path();
    ApiError::new(ErrorCategory::NotFound, format!("no route for {path}"))
        .into_response_for(ApiFlavor::from_path(path))
}
