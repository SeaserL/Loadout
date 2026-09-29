// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use loadout_protocol::Protocol;
use serde_json::json;

use crate::plane::DataPlane;
use crate::{ApiError, ApiFlavor, ClientId, ErrorCategory, proxy};

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub plane: Arc<DataPlane>,
    pub max_body: usize,
}

/// `GET /v1/models`: OpenAI shape by default, Anthropic shape when the client sends
/// `anthropic-version` (Anthropic SDKs do).
pub(crate) async fn list_models(State(app): State<AppState>, headers: HeaderMap) -> Response {
    let table = app.plane.routes.load();
    let names = table.model_names();
    if headers.contains_key("anthropic-version") {
        let data: Vec<_> = names
            .iter()
            .map(|n| json!({ "type": "model", "id": n, "display_name": n, "created_at": "1970-01-01T00:00:00Z" }))
            .collect();
        return Json(json!({
            "data": data,
            "has_more": false,
            "first_id": names.first(),
            "last_id": names.last(),
        }))
        .into_response();
    }
    let data: Vec<_> = names
        .iter()
        .map(|n| {
            json!({
                "id": n,
                "object": "model",
                "created": 0,
                "owned_by": n.split('/').next().unwrap_or("loadout"),
            })
        })
        .collect();
    Json(json!({ "object": "list", "data": data })).into_response()
}

pub(crate) async fn chat(
    State(app): State<AppState>,
    Extension(client): Extension<ClientId>,
    req: Request,
) -> Response {
    proxy::handle(app.plane, app.max_body, Protocol::Chat, client, req).await
}

pub(crate) async fn responses(
    State(app): State<AppState>,
    Extension(client): Extension<ClientId>,
    req: Request,
) -> Response {
    proxy::handle(app.plane, app.max_body, Protocol::Responses, client, req).await
}

pub(crate) async fn messages(
    State(app): State<AppState>,
    Extension(client): Extension<ClientId>,
    req: Request,
) -> Response {
    proxy::handle(app.plane, app.max_body, Protocol::Messages, client, req).await
}

pub(crate) async fn not_found(req: Request) -> Response {
    let path = req.uri().path();
    ApiError::new(ErrorCategory::NotFound, format!("no route for {path}"))
        .into_response_for(ApiFlavor::from_path(path))
}
