use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use tonic::transport::Channel;

use agent_proto::agent::{
    BuildImageRequest, build_image_event::Stage, host_agent_client::HostAgentClient,
};

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
        .route("/api/admin/images", get(list_images).post(build_image))
        .route("/api/admin/images/{id}", delete(delete_image))
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

// ── Images ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ImageResponse {
    id: String,
    name: String,
    tag: String,
    source: String,
    status: String,
    size_bytes: i64,
    error: Option<String>,
    created_at: i64,
}

impl From<db::ImageRow> for ImageResponse {
    fn from(i: db::ImageRow) -> Self {
        Self {
            id: i.id,
            name: i.name,
            tag: i.tag,
            source: i.source,
            status: i.status,
            size_bytes: i.size_bytes,
            error: i.error,
            created_at: i.created_at,
        }
    }
}

async fn list_images(_admin: auth::AdminId, State(state): State<AdminState>) -> impl IntoResponse {
    match db::list_images(&state.pool).await {
        Ok(images) => Json(
            images
                .into_iter()
                .map(ImageResponse::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct BuildImageBody {
    source: String,
    name: String,
    tag: Option<String>,
}

async fn build_image(
    _admin: auth::AdminId,
    State(state): State<AdminState>,
    Json(body): Json<BuildImageBody>,
) -> impl IntoResponse {
    let tag = body.tag.unwrap_or_else(|| "latest".into());

    // pick any active host to run the build on
    let host = match db::list_hosts(&state.pool).await {
        Ok(hosts) => match hosts.into_iter().find(|h| h.status == "active") {
            Some(h) => h,
            None => {
                return (StatusCode::SERVICE_UNAVAILABLE, "no active hosts available")
                    .into_response();
            }
        },
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let image_id = uuid::Uuid::new_v4().to_string();

    let image = match db::create_image(&state.pool, &image_id, &body.name, &tag, &body.source).await
    {
        Ok(i) => i,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let pool = state.pool.clone();
    let source = body.source.clone();
    let name = body.name.clone();

    tokio::spawn(async move {
        let channel = match Channel::from_shared(host.address.clone())
            .and_then(|e| Ok(e))
            .map_err(|e| anyhow::anyhow!(e))
        {
            Ok(ep) => match ep.connect().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("build_image: connect to agent: {e}");
                    let _ = db::update_image_error(&pool, &image_id, &e.to_string()).await;
                    return;
                }
            },
            Err(e) => {
                tracing::error!("build_image: bad agent address: {e}");
                let _ = db::update_image_error(&pool, &image_id, &e.to_string()).await;
                return;
            }
        };

        let mut agent = HostAgentClient::new(channel);
        let req = BuildImageRequest {
            image_id: image_id.clone(),
            source: source.clone(),
            name: name.clone(),
            tag: tag.clone(),
        };

        let mut stream = match agent.build_image(req).await {
            Ok(r) => r.into_inner(),
            Err(e) => {
                tracing::error!("build_image: rpc error: {e}");
                let _ = db::update_image_error(&pool, &image_id, &e.to_string()).await;
                return;
            }
        };

        loop {
            match stream.message().await {
                Ok(Some(ev)) => {
                    let stage = Stage::try_from(ev.stage).unwrap_or(Stage::Error);
                    tracing::info!(
                        image_id = %image_id,
                        stage = ?stage,
                        message = %ev.message,
                        "build progress"
                    );
                    match stage {
                        Stage::Done => {
                            let _ = db::update_image_ready(&pool, &image_id, ev.size_bytes).await;
                            tracing::info!(image_id = %image_id, "image build complete");
                            return;
                        }
                        Stage::Error => {
                            let _ = db::update_image_error(&pool, &image_id, &ev.message).await;
                            tracing::error!(image_id = %image_id, error = %ev.message, "image build failed");
                            return;
                        }
                        _ => {}
                    }
                }
                Ok(None) => {
                    // stream ended without Done/Error — treat as error
                    let _ =
                        db::update_image_error(&pool, &image_id, "build stream ended unexpectedly")
                            .await;
                    return;
                }
                Err(e) => {
                    let _ = db::update_image_error(&pool, &image_id, &e.to_string()).await;
                    tracing::error!(image_id = %image_id, "build stream error: {e}");
                    return;
                }
            }
        }
    });

    (StatusCode::ACCEPTED, Json(ImageResponse::from(image))).into_response()
}

async fn delete_image(
    _admin: auth::AdminId,
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match db::get_image(&state.pool, &id).await {
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Ok(Some(img)) if img.status == "building" => {
            return (StatusCode::CONFLICT, "image is currently building").into_response();
        }
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Ok(Some(_)) => {}
    }
    match db::delete_image(&state.pool, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
