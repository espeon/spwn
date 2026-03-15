use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, post},
};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct GatewayState {
    pub ops: Arc<dyn api::VmOps>,
    pub gateway_secret: Option<String>,
}

pub fn router(state: GatewayState) -> Router {
    Router::new()
        .route("/internal/gateway/vms/create", post(create_vm))
        .route("/internal/gateway/vms/{id}/start", post(start_vm))
        .route("/internal/gateway/vms/{id}/stop", post(stop_vm))
        .route("/internal/gateway/vms/{id}", delete(delete_vm))
        .with_state(state)
}

fn check_secret(state: &GatewayState, headers: &HeaderMap) -> bool {
    let secret = match &state.gateway_secret {
        Some(s) => s,
        None => return false,
    };
    let auth = match headers.get("authorization").and_then(|v| v.to_str().ok()) {
        Some(v) => v,
        None => return false,
    };
    let token = auth.strip_prefix("Bearer ").unwrap_or(auth);
    token == secret
}

#[derive(Deserialize)]
struct AccountQuery {
    account_id: String,
}

#[derive(Deserialize)]
struct CreateVmBody {
    name: String,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    vcpus: Option<i64>,
    #[serde(default)]
    memory_mb: Option<i32>,
}

#[derive(Serialize)]
struct GatewayVmItem {
    id: String,
    name: String,
    status: String,
    subdomain: String,
}

async fn create_vm(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Query(q): Query<AccountQuery>,
    Json(body): Json<CreateVmBody>,
) -> impl IntoResponse {
    if !check_secret(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let req = api::CreateVmRequest {
        name: body.name,
        image: body.image.unwrap_or_else(|| "ubuntu".into()),
        vcpus: body.vcpus.unwrap_or(1000),
        memory_mb: body.memory_mb.unwrap_or(512),
        disk_mb: 5120,
        bandwidth_mbps: 100,
        exposed_port: 8080,
        placement_strategy: "best_fit".into(),
        required_labels: None,
        region: None,
        namespace_id: None,
    };
    match state.ops.create_vm(q.account_id, req).await {
        Ok(vm) => Json(GatewayVmItem {
            id: vm.id,
            name: vm.name,
            status: vm.status,
            subdomain: vm.subdomain,
        })
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn start_vm(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(_q): Query<AccountQuery>,
) -> impl IntoResponse {
    if !check_secret(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.ops.start_vm(&id).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn stop_vm(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(_q): Query<AccountQuery>,
) -> impl IntoResponse {
    if !check_secret(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.ops.stop_vm(&id).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn delete_vm(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(_q): Query<AccountQuery>,
) -> impl IntoResponse {
    if !check_secret(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.ops.delete_vm(&id).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
