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
    region: Option<String>,
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
            region: v.region,
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
    build_log: String,
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
            build_log: i.build_log,
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
                    let stage_label = match stage {
                        Stage::Pulling => "pull",
                        Stage::Exporting => "export",
                        Stage::Squashing => "squash",
                        Stage::Done => "done",
                        Stage::Error => "error",
                    };
                    let log_line = format!("[{stage_label}] {}\n", ev.message);
                    let _ = db::append_image_log(&pool, &image_id, &log_line).await;
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
                    let msg = "build stream ended unexpectedly";
                    let _ = db::append_image_log(&pool, &image_id, &format!("[error] {msg}\n")).await;
                    let _ = db::update_image_error(&pool, &image_id, msg).await;
                    return;
                }
                Err(e) => {
                    let _ = db::append_image_log(&pool, &image_id, &format!("[error] {e}\n")).await;
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::{Extension, Router, body::Body, http::{Request, StatusCode, header}};
    use http_body_util::BodyExt;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;
    use tower::ServiceExt;

    use router_sync::CaddyClient;

    use super::{AdminState, router};
    use crate::caddy_router::CaddyRouter;

    async fn setup_db() -> (testcontainers::ContainerAsync<Postgres>, db::PgPool) {
        let container = Postgres::default().start().await.expect("start postgres");
        let port = container.get_host_port_ipv4(5432).await.expect("get port");
        let url = format!("postgres://postgres:postgres@localhost:{port}/postgres");
        let pool = db::connect(&url).await.expect("connect");
        db::migrate(&pool).await.expect("migrate");
        (container, pool)
    }

    fn mock_caddy() -> CaddyRouter {
        CaddyRouter::new(
            CaddyClient::new("http://127.0.0.1:1", std::path::PathBuf::from("/tmp")),
            HashMap::new(),
        )
    }

    fn admin_app(pool: db::PgPool, caddy: CaddyRouter) -> Router {
        let state = AdminState { pool: pool.clone(), caddy };
        router(state).layer(Extension(pool))
    }

    async fn create_superadmin_session(pool: &db::PgPool) -> (String, String) {
        let account_id = uuid::Uuid::new_v4().to_string();
        let session_id = uuid::Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        db::create_account(
            pool,
            &db::NewAccount {
                id: account_id.clone(),
                email: format!("{account_id}@test.com"),
                password_hash: "x".into(),
                username: "adminuser".into(),
                created_at: now,
            },
        )
        .await
        .expect("create account");

        sqlx::query("UPDATE accounts SET role = 'superadmin' WHERE id = $1")
            .bind(&account_id)
            .execute(pool)
            .await
            .expect("promote to superadmin");

        db::create_session(
            pool,
            &db::NewSession {
                id: session_id.clone(),
                account_id: account_id.clone(),
                created_at: now,
                expires_at: now + 86400,
            },
        )
        .await
        .expect("create session");

        (account_id, session_id)
    }

    async fn create_regular_session(pool: &db::PgPool) -> String {
        let account_id = uuid::Uuid::new_v4().to_string();
        let session_id = uuid::Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        db::create_account(
            pool,
            &db::NewAccount {
                id: account_id.clone(),
                email: format!("{account_id}@test.com"),
                password_hash: "x".into(),
                username: "regularuser".into(),
                created_at: now,
            },
        )
        .await
        .expect("create account");

        db::create_session(
            pool,
            &db::NewSession {
                id: session_id.clone(),
                account_id,
                created_at: now,
                expires_at: now + 86400,
            },
        )
        .await
        .expect("create session");

        session_id
    }

    async fn body_json(body: Body) -> serde_json::Value {
        let bytes = body.collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    // ── auth guard ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn unauthenticated_returns_401() {
        let (_c, pool) = setup_db().await;
        let app = admin_app(pool, mock_caddy());

        let resp = app
            .oneshot(Request::get("/api/admin/hosts").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn non_admin_returns_403() {
        let (_c, pool) = setup_db().await;
        let session_id = create_regular_session(&pool).await;
        let app = admin_app(pool, mock_caddy());

        let resp = app
            .oneshot(
                Request::get("/api/admin/hosts")
                    .header(header::COOKIE, format!("session_id={session_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // ── list_hosts ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_hosts_returns_empty() {
        let (_c, pool) = setup_db().await;
        let (_admin_id, session_id) = create_superadmin_session(&pool).await;
        let app = admin_app(pool, mock_caddy());

        let resp = app
            .oneshot(
                Request::get("/api/admin/hosts")
                    .header(header::COOKIE, format!("session_id={session_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp.into_body()).await, serde_json::json!([]));
    }

    #[tokio::test]
    async fn list_hosts_returns_host_data() {
        let (_c, pool) = setup_db().await;
        let (_admin_id, session_id) = create_superadmin_session(&pool).await;

        db::upsert_host(
            &pool,
            &db::NewHost {
                id: "host-1".into(),
                name: "node-1".into(),
                address: "http://node-1:4000".into(),
                vcpu_total: 8000,
                mem_total_mb: 16384,
                images_dir: "/images".into(),
                overlay_dir: "/overlay".into(),
                snapshot_dir: "/snapshots".into(),
                kernel_path: "/vmlinux".into(),
                snapshot_addr: "http://node-1:8080".into(),
            },
        )
        .await
        .expect("upsert host");

        let app = admin_app(pool, mock_caddy());

        let resp = app
            .oneshot(
                Request::get("/api/admin/hosts")
                    .header(header::COOKIE, format!("session_id={session_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let val = body_json(resp.into_body()).await;
        assert_eq!(val.as_array().unwrap().len(), 1);
        assert_eq!(val[0]["name"], "node-1");
    }

    // ── list_admin_vms ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_admin_vms_returns_empty() {
        let (_c, pool) = setup_db().await;
        let (_admin_id, session_id) = create_superadmin_session(&pool).await;
        let app = admin_app(pool, mock_caddy());

        let resp = app
            .oneshot(
                Request::get("/api/admin/vms")
                    .header(header::COOKIE, format!("session_id={session_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp.into_body()).await, serde_json::json!([]));
    }

    // ── set_host_status ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn set_host_status_invalid_returns_400() {
        let (_c, pool) = setup_db().await;
        let (_admin_id, session_id) = create_superadmin_session(&pool).await;
        let app = admin_app(pool, mock_caddy());

        let resp = app
            .oneshot(
                Request::post("/api/admin/hosts/h1/status")
                    .header(header::COOKIE, format!("session_id={session_id}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({"status": "bad-value"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn set_host_status_valid_returns_204() {
        let (_c, pool) = setup_db().await;
        let (_admin_id, session_id) = create_superadmin_session(&pool).await;

        for status in ["active", "draining", "offline"] {
            let resp = admin_app(pool.clone(), mock_caddy())
                .oneshot(
                    Request::post("/api/admin/hosts/nonexistent-host/status")
                        .header(header::COOKIE, format!("session_id={session_id}"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            serde_json::json!({"status": status}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::NO_CONTENT, "status={status}");
        }
    }

    // ── migrate_vm ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn migrate_vm_returns_202() {
        let (_c, pool) = setup_db().await;
        let (_admin_id, session_id) = create_superadmin_session(&pool).await;
        let app = admin_app(pool, mock_caddy());

        let resp = app
            .oneshot(
                Request::post("/api/admin/vms/some-vm-id/migrate")
                    .header(header::COOKIE, format!("session_id={session_id}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({"target_host_id": "target-host"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }

    // ── images ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_images_returns_empty() {
        let (_c, pool) = setup_db().await;
        let (_admin_id, session_id) = create_superadmin_session(&pool).await;
        let app = admin_app(pool, mock_caddy());

        let resp = app
            .oneshot(
                Request::get("/api/admin/images")
                    .header(header::COOKIE, format!("session_id={session_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp.into_body()).await, serde_json::json!([]));
    }

    #[tokio::test]
    async fn build_image_no_active_hosts_returns_503() {
        let (_c, pool) = setup_db().await;
        let (_admin_id, session_id) = create_superadmin_session(&pool).await;
        let app = admin_app(pool, mock_caddy());

        let resp = app
            .oneshot(
                Request::post("/api/admin/images")
                    .header(header::COOKIE, format!("session_id={session_id}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({"source": "ubuntu:22.04", "name": "ubuntu"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn delete_image_not_found_returns_404() {
        let (_c, pool) = setup_db().await;
        let (_admin_id, session_id) = create_superadmin_session(&pool).await;
        let app = admin_app(pool, mock_caddy());

        let resp = app
            .oneshot(
                Request::delete("/api/admin/images/nonexistent")
                    .header(header::COOKIE, format!("session_id={session_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_image_building_returns_409() {
        let (_c, pool) = setup_db().await;
        let (_admin_id, session_id) = create_superadmin_session(&pool).await;

        let image_id = uuid::Uuid::new_v4().to_string();
        db::create_image(&pool, &image_id, "ubuntu", "22.04", "ubuntu:22.04")
            .await
            .expect("create image");

        let app = admin_app(pool, mock_caddy());

        let resp = app
            .oneshot(
                Request::delete(format!("/api/admin/images/{image_id}"))
                    .header(header::COOKIE, format!("session_id={session_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }
}
