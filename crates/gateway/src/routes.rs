// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use axum::extract::{Path, Request, State};
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

/// `GET /e/<endpoint>/v1/models`: the endpoint's bare model names (ADR-015).
pub(crate) async fn endpoint_models(
    State(app): State<AppState>,
    Path(endpoint): Path<String>,
    headers: HeaderMap,
) -> Response {
    let table = app.plane.routes.load();
    let anthropic = headers.contains_key("anthropic-version");
    let flavor = if anthropic {
        ApiFlavor::Anthropic
    } else {
        ApiFlavor::OpenAi
    };
    let Some(models) = table.endpoint_models(&endpoint) else {
        return ApiError::new(
            ErrorCategory::NotFound,
            format!("endpoint {endpoint:?} does not exist or is turned off"),
        )
        .into_response_for(flavor);
    };
    let owner = table.endpoint_provider(&endpoint).unwrap_or("loadout");
    if anthropic {
        let data: Vec<_> = models
            .iter()
            .map(|n| json!({ "type": "model", "id": n, "display_name": n, "created_at": "1970-01-01T00:00:00Z" }))
            .collect();
        return Json(json!({
            "data": data,
            "has_more": false,
            "first_id": models.first(),
            "last_id": models.last(),
        }))
        .into_response();
    }
    let data: Vec<_> = models
        .iter()
        .map(|n| json!({ "id": n, "object": "model", "created": 0, "owned_by": owner }))
        .collect();
    Json(json!({ "object": "list", "data": data })).into_response()
}

/// A request to a dedicated endpoint: routing rounds until one answers or the endpoint's
/// recovery wait runs out (ADR-015 §3).
async fn endpoint_proxy(
    app: AppState,
    endpoint: String,
    ingress: Protocol,
    client: ClientId,
    req: Request,
) -> Response {
    let started = std::time::Instant::now();
    let ts_ms = now_ms();
    let flavor = ApiFlavor::for_protocol(ingress);
    let (headers, bytes) = match proxy::read_body(req, app.max_body, flavor).await {
        Ok(v) => v,
        Err(resp) => return *resp,
    };
    let wait_ms = i64::from(app.plane.routes.load().endpoint_wait_secs(&endpoint)) * 1000;
    let deadline = ts_ms + wait_ms;
    loop {
        let now = now_ms();
        let scope = proxy::Scope {
            name: &endpoint,
            retry_until_ms: (now < deadline).then_some(deadline),
        };
        let resp = proxy::handle_bytes(
            app.plane.clone(),
            ingress,
            client.clone(),
            &headers,
            &bytes,
            Some(scope),
            started,
            ts_ms,
        )
        .await;
        let Some(proxy::RetryAt(at)) = resp.extensions().get::<proxy::RetryAt>().copied() else {
            return resp;
        };
        // Just after the recovery time, and never past the deadline.
        let wake = (at + 100).min(deadline).max(now + 50);
        tokio::time::sleep(std::time::Duration::from_millis(
            u64::try_from(wake - now).unwrap_or(50),
        ))
        .await;
    }
}

pub(crate) async fn endpoint_chat(
    State(app): State<AppState>,
    Path(endpoint): Path<String>,
    Extension(client): Extension<ClientId>,
    req: Request,
) -> Response {
    endpoint_proxy(app, endpoint, Protocol::Chat, client, req).await
}

pub(crate) async fn endpoint_responses(
    State(app): State<AppState>,
    Path(endpoint): Path<String>,
    Extension(client): Extension<ClientId>,
    req: Request,
) -> Response {
    endpoint_proxy(app, endpoint, Protocol::Responses, client, req).await
}

pub(crate) async fn endpoint_messages(
    State(app): State<AppState>,
    Path(endpoint): Path<String>,
    Extension(client): Extension<ClientId>,
    req: Request,
) -> Response {
    endpoint_proxy(app, endpoint, Protocol::Messages, client, req).await
}

pub(crate) async fn not_found(req: Request) -> Response {
    let path = req.uri().path();
    ApiError::new(ErrorCategory::NotFound, format!("no route for {path}"))
        .into_response_for(ApiFlavor::from_path(path))
}
