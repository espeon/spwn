use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

// VmManager is defined in vm-manager, but the API crate needs to call into it.
// We use a trait to avoid a circular dependency.
#[async_trait::async_trait]
pub trait VmOps: Send + Sync {
    async fn create_vm(&self, req: CreateVmRequest) -> anyhow::Result<db::VmRow>;
    async fn start_vm(&self, id: &str) -> anyhow::Result<()>;
    async fn stop_vm(&self, id: &str) -> anyhow::Result<()>;
    async fn delete_vm(&self, id: &str) -> anyhow::Result<()>;
    async fn get_vm(&self, id: &str) -> anyhow::Result<Option<db::VmRow>>;
    async fn list_vms(&self, account_id: &str) -> anyhow::Result<Vec<db::VmRow>>;
}

#[derive(Debug, Deserialize)]
pub struct CreateVmRequest {
    pub name: String,
    #[serde(default = "default_vcores")]
    pub vcores: i32,
    #[serde(default = "default_memory")]
    pub memory_mb: i32,
    #[serde(default = "default_port")]
    pub exposed_port: i32,
}

fn default_vcores() -> i32 { 2 }
fn default_memory() -> i32 { 512 }
fn default_port() -> i32 { 8080 }

#[derive(Serialize)]
struct VmResponse {
    id: String,
    name: String,
    status: String,
    subdomain: String,
    vcores: i32,
    memory_mb: i32,
    ip_address: String,
    exposed_port: i32,
}

impl From<db::VmRow> for VmResponse {
    fn from(v: db::VmRow) -> Self {
        Self {
            id: v.id,
            name: v.name,
            status: v.status,
            subdomain: v.subdomain,
            vcores: v.vcores,
            memory_mb: v.memory_mb,
            ip_address: v.ip_address,
            exposed_port: v.exposed_port,
        }
    }
}

type AppState = Arc<dyn VmOps>;

pub fn router(ops: Arc<dyn VmOps>) -> Router {
    Router::new()
        .route("/api/vms", get(list_vms).post(create_vm))
        .route("/api/vms/{id}", get(get_vm).delete(delete_vm))
        .route("/api/vms/{id}/start", post(start_vm))
        .route("/api/vms/{id}/stop", post(stop_vm))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(ops)
}

async fn list_vms(State(ops): State<AppState>) -> impl IntoResponse {
    // phase 3: hardcoded dev account until auth is implemented
    match ops.list_vms("dev").await {
        Ok(vms) => Json(vms.into_iter().map(VmResponse::from).collect::<Vec<_>>()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn create_vm(
    State(ops): State<AppState>,
    Json(req): Json<CreateVmRequest>,
) -> impl IntoResponse {
    match ops.create_vm(req).await {
        Ok(vm) => (StatusCode::CREATED, Json(VmResponse::from(vm))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_vm(State(ops): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    match ops.get_vm(&id).await {
        Ok(Some(vm)) => Json(VmResponse::from(vm)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn delete_vm(State(ops): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    match ops.delete_vm(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn start_vm(State(ops): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    tokio::spawn(async move {
        if let Err(e) = ops.start_vm(&id).await {
            tracing::error!("start_vm {id} failed: {e:#}");
        }
    });
    StatusCode::ACCEPTED
}

async fn stop_vm(State(ops): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    tokio::spawn(async move {
        if let Err(e) = ops.stop_vm(&id).await {
            tracing::error!("stop_vm {id} failed: {e:#}");
        }
    });
    StatusCode::ACCEPTED
}
