// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use loadout_protocol::Protocol;
use loadout_router::{ListingKind, ModelListing, RouteTable, now_ms};
use serde_json::{Value, json};

use crate::plane::DataPlane;
use crate::{ApiError, ApiFlavor, ClientId, ErrorCategory, proxy};

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub plane: Arc<DataPlane>,
    pub max_body: usize,
}

/// `GET /v1/models`: OpenAI shape by default, Anthropic shape when the client sends
/// `anthropic-version` (Anthropic SDKs do).
pub(crate) async fn list_models(
    State(app): State<AppState>,
    Extension(client): Extension<ClientId>,
    headers: HeaderMap,
) -> Response {
    let table = app.plane.routes.load();
    if headers.contains_key("anthropic-version") {
        let mut names = table.model_names();
        // Only what this client key may use (ADR-010 §4).
        if let Some(policy) = app.plane.policies.get(&client) {
            names.retain(|n| policy.allows(n));
        }
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
    let policy = app.plane.policies.get(&client);
    let now = now_ms();
    let data: Vec<_> = table
        .listing()
        .iter()
        .filter(|entry| policy.as_deref().is_none_or(|p| p.allows(&entry.name)))
        .map(|entry| openai_model(&table, entry, now))
        .collect();
    Json(json!({ "object": "list", "data": data })).into_response()
}

/// OpenAI model object plus optional Loadout fields (clients ignore unknown fields):
/// `context_window` and `loadout: {kind, tier, state, input_price, output_price, …}`.
/// Built from the in-memory route table only.
fn openai_model(table: &RouteTable, entry: &ModelListing<'_>, now: i64) -> Value {
    let n = &entry.name;
    let available = entry
        .targets
        .iter()
        .filter(|t| t.health.is_available(now))
        .count();
    // Virtual models and pools describe their current head (first available target).
    let head = entry
        .targets
        .iter()
        .find(|t| t.health.is_available(now))
        .or(entry.targets.first());
    let mut loadout = json!({ "kind": entry.kind.as_str() });
    if let Some(t) = head {
        let state = match entry.kind {
            ListingKind::Exact => t.health.state().as_str(),
            _ if available > 0 => "AVAILABLE",
            _ => "UNAVAILABLE",
        };
        loadout["tier"] = json!(table.tier(t).as_str());
        loadout["state"] = json!(state);
        loadout["input_price"] = json!(t.prices.input);
        loadout["output_price"] = json!(t.prices.output);
        if entry.kind != ListingKind::Exact {
            loadout["head"] = json!(t.exact_name());
            loadout["targets"] = json!(entry.targets.len());
        }
    }
    let mut model = json!({
        "id": n,
        "object": "model",
        "created": 0,
        "owned_by": n.split('/').next().unwrap_or("loadout"),
        "loadout": loadout,
    });
    if let Some(cw) = head.and_then(|t| t.context_window) {
        model["context_window"] = json!(cw);
    }
    model
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
