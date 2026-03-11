use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};

use auth::AccountId;

#[async_trait::async_trait]
pub trait VmOps: Send + Sync {
    async fn create_vm(
        &self,
        account_id: String,
        req: CreateVmRequest,
    ) -> anyhow::Result<db::VmRow>;
    async fn clone_vm(
        &self,
        source_id: &str,
        account_id: &str,
        req: CloneVmRequest,
    ) -> anyhow::Result<db::VmRow>;
    async fn start_vm(&self, id: &str) -> anyhow::Result<()>;
    async fn stop_vm(&self, id: &str) -> anyhow::Result<()>;
    async fn delete_vm(&self, id: &str) -> anyhow::Result<()>;
    async fn get_vm(&self, id: &str) -> anyhow::Result<Option<db::VmRow>>;
    async fn list_vms(&self, account_id: &str) -> anyhow::Result<Vec<db::VmRow>>;
    async fn take_snapshot(
        &self,
        vm_id: &str,
        label: Option<String>,
    ) -> anyhow::Result<db::SnapshotRow>;
    async fn list_snapshots(&self, vm_id: &str) -> anyhow::Result<Vec<db::SnapshotRow>>;
    async fn delete_snapshot(&self, vm_id: &str, snap_id: &str) -> anyhow::Result<()>;
    async fn restore_snapshot(&self, vm_id: &str, snap_id: &str) -> anyhow::Result<()>;
    async fn change_username(&self, account_id: &str, new_username: &str) -> anyhow::Result<()>;
    async fn resize_resources(
        &self,
        vm_id: &str,
        account_id: &str,
        patch: VmResourcePatch,
    ) -> anyhow::Result<db::VmRow>;
    async fn update_vm(
        &self,
        vm_id: &str,
        account_id: &str,
        patch: VmPatch,
    ) -> anyhow::Result<db::VmRow>;
}

pub struct VmPatch {
    pub name: Option<String>,
    pub exposed_port: Option<i32>,
}

pub struct VmResourcePatch {
    pub vcpus: Option<i64>,
    pub memory_mb: Option<i32>,
    pub bandwidth_mbps: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct ChangeUsernameRequest {
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateVmRequest {
    pub name: String,
    #[serde(default = "default_image")]
    pub image: String,
    #[serde(default = "default_vcpus")]
    pub vcpus: i64,
    #[serde(default = "default_memory")]
    pub memory_mb: i32,
    #[serde(default = "default_disk")]
    pub disk_mb: i32,
    #[serde(default = "default_bandwidth")]
    pub bandwidth_mbps: i32,
    #[serde(default = "default_port")]
    pub exposed_port: i32,
    #[serde(default = "default_placement_strategy")]
    pub placement_strategy: String,
    #[serde(default)]
    pub required_labels: Option<serde_json::Value>,
}

fn default_image() -> String {
    "ubuntu".into()
}
fn default_vcpus() -> i64 {
    1000
}
fn default_memory() -> i32 {
    512
}
fn default_port() -> i32 {
    8080
}
fn default_disk() -> i32 {
    5120
}
fn default_bandwidth() -> i32 {
    100
}
fn default_placement_strategy() -> String {
    "best_fit".into()
}

#[derive(Debug, Deserialize)]
pub struct CloneVmRequest {
    pub name: String,
    #[serde(default)]
    pub include_memory: bool,
}

#[derive(Debug, Deserialize)]
pub struct SnapshotRequest {
    pub label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VmPatchRequest {
    pub name: Option<String>,
    pub exposed_port: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct VmResourcePatchRequest {
    pub vcpus: Option<i64>,
    pub memory_mb: Option<i32>,
    pub bandwidth_mbps: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct VmListQuery {
    pub name: Option<String>,
}

#[derive(Serialize)]
struct VmResponse {
    id: String,
    name: String,
    status: String,
    subdomain: String,
    vcpus: i64,
    memory_mb: i32,
    disk_mb: i32,
    bandwidth_mbps: i32,
    ip_address: String,
    exposed_port: i32,
    image: String,
    overlay_path: Option<String>,
    cloned_from: Option<String>,
    disk_usage_mb: i32,
}

impl From<db::VmRow> for VmResponse {
    fn from(v: db::VmRow) -> Self {
        let image = std::path::Path::new(&v.rootfs_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&v.rootfs_path)
            .to_string();
        Self {
            id: v.id,
            name: v.name,
            status: v.status,
            subdomain: v.subdomain,
            vcpus: v.vcpus,
            memory_mb: v.memory_mb,
            disk_mb: v.disk_mb,
            bandwidth_mbps: v.bandwidth_mbps,
            ip_address: v.ip_address,
            exposed_port: v.exposed_port,
            image,
            overlay_path: v.overlay_path,
            cloned_from: v.cloned_from,
            disk_usage_mb: v.disk_usage_mb,
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

#[derive(Serialize)]
struct VmEventResponse {
    id: i64,
    vm_id: String,
    event: String,
    metadata: Option<String>,
    created_at: i64,
}

impl From<db::VmEventRow> for VmEventResponse {
    fn from(e: db::VmEventRow) -> Self {
        Self {
            id: e.id,
            vm_id: e.vm_id,
            event: e.event,
            metadata: e.metadata,
            created_at: e.created_at,
        }
    }
}

type AppState = Arc<dyn VmOps>;

pub fn router(ops: Arc<dyn VmOps>) -> Router {
    Router::new()
        .route("/api/vms", get(list_vms).post(create_vm))
        .route(
            "/api/vms/{id}",
            get(get_vm).delete(delete_vm).patch(patch_vm),
        )
        .route("/api/vms/{id}/start", post(start_vm))
        .route("/api/vms/{id}/stop", post(stop_vm))
        .route("/api/vms/{id}/snapshot", post(take_snapshot))
        .route("/api/vms/{id}/snapshots", get(list_snapshots))
        .route("/api/vms/{id}/snapshots/{snap_id}", delete(delete_snapshot))
        .route("/api/vms/{id}/clone", post(clone_vm))
        .route("/api/vms/{id}/resources", post(resize_resources))
        .route("/api/vms/{id}/restore/{snap_id}", post(restore_snapshot))
        .route("/api/vms/{id}/events", get(list_vm_events))
        .route("/api/account/username", post(change_username))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(ops)
}

async fn change_username(
    State(ops): State<AppState>,
    account_id: AccountId,
    Json(req): Json<ChangeUsernameRequest>,
) -> impl IntoResponse {
    match ops.change_username(&account_id.0, &req.username).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") || msg.contains("already taken")
            {
                (StatusCode::CONFLICT, "username already taken").into_response()
            } else if msg.contains("invalid username") {
                (StatusCode::BAD_REQUEST, msg).into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
        }
    }
}

async fn list_vms(
    State(ops): State<AppState>,
    Extension(pool): Extension<db::PgPool>,
    account_id: AccountId,
    Query(query): Query<VmListQuery>,
) -> impl IntoResponse {
    if let Some(name) = query.name {
        return match db::get_vm_by_name(&pool, &account_id.0, &name).await {
            Ok(Some(vm)) => Json(vec![VmResponse::from(vm)]).into_response(),
            Ok(None) => Json(Vec::<VmResponse>::new()).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        };
    }
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

async fn clone_vm(
    State(ops): State<AppState>,
    account_id: AccountId,
    Path(id): Path<String>,
    Json(req): Json<CloneVmRequest>,
) -> impl IntoResponse {
    match ops.clone_vm(&id, &account_id.0, req).await {
        Ok(vm) => (StatusCode::CREATED, Json(VmResponse::from(vm))).into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("quota") || msg.contains("limit") {
                (StatusCode::UNPROCESSABLE_ENTITY, msg).into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
        }
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
        Ok(snaps) => Json(
            snaps
                .into_iter()
                .map(SnapshotResponse::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
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

#[derive(Debug, Deserialize)]
struct EventsQuery {
    #[serde(default = "default_event_limit")]
    limit: i64,
    before: Option<i64>,
}

fn default_event_limit() -> i64 {
    50
}

async fn list_vm_events(
    State(ops): State<AppState>,
    Extension(pool): Extension<db::PgPool>,
    account_id: AccountId,
    Path(id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> impl IntoResponse {
    let vm = match ops.get_vm(&id).await {
        Ok(Some(v)) => v,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    if vm.account_id != account_id.0 {
        return StatusCode::FORBIDDEN.into_response();
    }
    let limit = query.limit.clamp(1, 100);
    match db::list_vm_events(&pool, &id, limit, query.before).await {
        Ok(events) => Json(
            events
                .into_iter()
                .map(VmEventResponse::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn patch_vm(
    State(ops): State<AppState>,
    account_id: AccountId,
    Path(id): Path<String>,
    Json(body): Json<VmPatchRequest>,
) -> impl IntoResponse {
    let vm = match ops.get_vm(&id).await {
        Ok(Some(v)) => v,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    if vm.account_id != account_id.0 {
        return StatusCode::FORBIDDEN.into_response();
    }
    let p = VmPatch {
        name: body.name,
        exposed_port: body.exposed_port,
    };
    match ops.update_vm(&id, &account_id.0, p).await {
        Ok(updated) => Json(VmResponse::from(updated)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn resize_resources(
    State(ops): State<AppState>,
    account_id: AccountId,
    Path(id): Path<String>,
    Json(body): Json<VmResourcePatchRequest>,
) -> impl IntoResponse {
    let vm = match ops.get_vm(&id).await {
        Ok(Some(v)) => v,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    if vm.account_id != account_id.0 {
        return StatusCode::FORBIDDEN.into_response();
    }
    let p = VmResourcePatch {
        vcpus: body.vcpus,
        memory_mb: body.memory_mb,
        bandwidth_mbps: body.bandwidth_mbps,
    };
    match ops.resize_resources(&id, &account_id.0, p).await {
        Ok(updated) => Json(VmResponse::from(updated)).into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("restart required") {
                (StatusCode::UNPROCESSABLE_ENTITY, msg).into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
        }
    }
}
