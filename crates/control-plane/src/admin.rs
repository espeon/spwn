use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{caddy_router::CaddyRouter, migration};

#[derive(Clone)]
pub struct AdminState {
    pub pool: db::PgPool,
    pub caddy: CaddyRouter,
}

pub fn router(state: AdminState) -> Router {
    Router::new()
        .route("/api/admin/hosts", get(list_hosts))
        .route("/api/admin/hosts/{id}/status", post(set_host_status))
        .route("/api/admin/vms", get(list_admin_vms))
        .route("/api/admin/vms/{vm_id}/migrate", post(migrate_vm))
        .with_state(state)
}

#[derive(Serialize)]
struct HostResponse {
    id: String,
    name: String,
    address: String,
    status: String,
    vcpu_total: i64,
    vcpu_used: i64,
    mem_total_mb: i32,
    mem_used_mb: i32,
    labels: serde_json::Value,
    snapshot_addr: String,
    last_seen_at: i64,
}

impl From<db::HostRow> for HostResponse {
    fn from(h: db::HostRow) -> Self {
        Self {
            id: h.id,
            name: h.name,
            address: h.address,
            status: h.status,
            vcpu_total: h.vcpu_total,
            vcpu_used: h.vcpu_used,
            mem_total_mb: h.mem_total_mb,
            mem_used_mb: h.mem_used_mb,
            labels: h.labels,
            snapshot_addr: h.snapshot_addr,
            last_seen_at: h.last_seen_at,
        }
    }
}

async fn list_hosts(_admin: auth::AdminId, State(state): State<AdminState>) -> impl IntoResponse {
    match db::list_hosts(&state.pool).await {
        Ok(hosts) => Json(
            hosts
                .into_iter()
                .map(HostResponse::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Serialize)]
struct AdminVmResponse {
    id: String,
    name: String,
    status: String,
    host_id: Option<String>,
    account_id: String,
    username: String,
    vcpus: i64,
    memory_mb: i32,
    disk_usage_mb: i32,
    subdomain: String,
}

impl From<db::AdminVmRecord> for AdminVmResponse {
    fn from(v: db::AdminVmRecord) -> Self {
        Self {
            id: v.id,
            name: v.name,
            status: v.status,
            host_id: v.host_id,
            account_id: v.account_id,
            username: v.username,
            vcpus: v.vcpus,
            memory_mb: v.memory_mb,
            disk_usage_mb: v.disk_usage_mb,
            subdomain: v.subdomain,
        }
    }
}

async fn list_admin_vms(
    _admin: auth::AdminId,
    State(state): State<AdminState>,
) -> impl IntoResponse {
    match db::list_all_vms_admin(&state.pool).await {
        Ok(vms) => Json(
            vms.into_iter()
                .map(AdminVmResponse::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct SetStatusRequest {
    status: String,
}

async fn set_host_status(
    _admin: auth::AdminId,
    State(state): State<AdminState>,
    Path(id): Path<String>,
    Json(body): Json<SetStatusRequest>,
) -> impl IntoResponse {
    match body.status.as_str() {
        "active" | "draining" | "offline" => {}
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "status must be active, draining, or offline",
            )
                .into_response();
        }
    }
    match db::set_host_status(&state.pool, &id, &body.status).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct MigrateRequest {
    target_host_id: String,
}

async fn migrate_vm(
    _admin: auth::AdminId,
    State(state): State<AdminState>,
    Path(vm_id): Path<String>,
    Json(body): Json<MigrateRequest>,
) -> impl IntoResponse {
    let pool = state.pool.clone();
    let caddy = state.caddy.clone();
    let target = body.target_host_id.clone();
    tokio::spawn(async move {
        if let Err(e) = migration::migrate_vm(&pool, &caddy, &vm_id, &target).await {
            tracing::error!("admin migrate {vm_id} → {target}: {e}");
        }
    });
    StatusCode::ACCEPTED.into_response()
}
