use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};

use auth::AccountId;

#[async_trait::async_trait]
pub trait VmOps: Send + Sync {
    async fn create_vm(&self, account_id: String, req: CreateVmRequest) -> anyhow::Result<db::VmRow>;
    async fn start_vm(&self, id: &str) -> anyhow::Result<()>;
    async fn stop_vm(&self, id: &str) -> anyhow::Result<()>;
    async fn delete_vm(&self, id: &str) -> anyhow::Result<()>;
    async fn get_vm(&self, id: &str) -> anyhow::Result<Option<db::VmRow>>;
    async fn list_vms(&self, account_id: &str) -> anyhow::Result<Vec<db::VmRow>>;
    async fn take_snapshot(&self, vm_id: &str, label: Option<String>) -> anyhow::Result<db::SnapshotRow>;
    async fn list_snapshots(&self, vm_id: &str) -> anyhow::Result<Vec<db::SnapshotRow>>;
    async fn delete_snapshot(&self, vm_id: &str, snap_id: &str) -> anyhow::Result<()>;
    async fn restore_snapshot(&self, vm_id: &str, snap_id: &str) -> anyhow::Result<()>;
}

#[derive(Debug, Deserialize)]
pub struct CreateVmRequest {
    pub name: String,
    #[serde(default = "default_image")]
    pub image: String,
    #[serde(default = "default_vcores")]
    pub vcores: i32,
    #[serde(default = "default_memory")]
    pub memory_mb: i32,
    #[serde(default = "default_port")]
    pub exposed_port: i32,
}

fn default_image() -> String { "ubuntu".into() }
fn default_vcores() -> i32 { 2 }
fn default_memory() -> i32 { 512 }
fn default_port() -> i32 { 8080 }

#[derive(Debug, Deserialize)]
pub struct SnapshotRequest {
    pub label: Option<String>,
}

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
    rootfs_path: String,
    overlay_path: Option<String>,
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
            rootfs_path: v.rootfs_path,
            overlay_path: v.overlay_path,
        }
    }
}

#[derive(Serialize)]
struct SnapshotResponse {
    id: String,
    vm_id: String,
    label: Option<String>,
    size_bytes: i64,
    created_at: i64,
}

impl From<db::SnapshotRow> for SnapshotResponse {
    fn from(s: db::SnapshotRow) -> Self {
        Self {
            id: s.id,
            vm_id: s.vm_id,
            label: s.label,
            size_bytes: s.size_bytes,
            created_at: s.created_at,
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
        .route("/api/vms/{id}/snapshot", post(take_snapshot))
        .route("/api/vms/{id}/snapshots", get(list_snapshots))
        .route("/api/vms/{id}/snapshots/{snap_id}", delete(delete_snapshot))
        .route("/api/vms/{id}/restore/{snap_id}", post(restore_snapshot))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(ops)
}

async fn list_vms(
    State(ops): State<AppState>,
    account_id: AccountId,
) -> impl IntoResponse {
    match ops.list_vms(&account_id.0).await {
        Ok(vms) => Json(vms.into_iter().map(VmResponse::from).collect::<Vec<_>>()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn create_vm(
    State(ops): State<AppState>,
    account_id: AccountId,
    Json(req): Json<CreateVmRequest>,
) -> impl IntoResponse {
    match ops.create_vm(account_id.0, req).await {
        Ok(vm) => (StatusCode::CREATED, Json(VmResponse::from(vm))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_vm(
    State(ops): State<AppState>,
    _account_id: AccountId,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match ops.get_vm(&id).await {
        Ok(Some(vm)) => Json(VmResponse::from(vm)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn delete_vm(
    State(ops): State<AppState>,
    _account_id: AccountId,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match ops.delete_vm(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn start_vm(
    State(ops): State<AppState>,
    _account_id: AccountId,
    Path(id): Path<String>,
) -> impl IntoResponse {
    tokio::spawn(async move {
        if let Err(e) = ops.start_vm(&id).await {
            tracing::error!("start_vm {id} failed: {e:#}");
        }
    });
    StatusCode::ACCEPTED
}

async fn stop_vm(
    State(ops): State<AppState>,
    _account_id: AccountId,
    Path(id): Path<String>,
) -> impl IntoResponse {
    tokio::spawn(async move {
        if let Err(e) = ops.stop_vm(&id).await {
            tracing::error!("stop_vm {id} failed: {e:#}");
        }
    });
    StatusCode::ACCEPTED
}

async fn take_snapshot(
    State(ops): State<AppState>,
    _account_id: AccountId,
    Path(id): Path<String>,
    body: Option<Json<SnapshotRequest>>,
) -> impl IntoResponse {
    let label = body.and_then(|b| b.label.clone());
    match ops.take_snapshot(&id, label).await {
        Ok(snap) => (StatusCode::CREATED, Json(SnapshotResponse::from(snap))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn list_snapshots(
    State(ops): State<AppState>,
    _account_id: AccountId,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match ops.list_snapshots(&id).await {
        Ok(snaps) => Json(snaps.into_iter().map(SnapshotResponse::from).collect::<Vec<_>>()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn delete_snapshot(
    State(ops): State<AppState>,
    _account_id: AccountId,
    Path((id, snap_id)): Path<(String, String)>,
) -> impl IntoResponse {
    match ops.delete_snapshot(&id, &snap_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn restore_snapshot(
    State(ops): State<AppState>,
    _account_id: AccountId,
    Path((id, snap_id)): Path<(String, String)>,
) -> impl IntoResponse {
    tokio::spawn(async move {
        if let Err(e) = ops.restore_snapshot(&id, &snap_id).await {
            tracing::error!("restore_snapshot {id}/{snap_id} failed: {e:#}");
        }
    });
    StatusCode::ACCEPTED
}
