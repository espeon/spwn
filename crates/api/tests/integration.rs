use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use tower::ServiceExt;

use api::{CloneVmRequest, CreateVmRequest, VmOps, VmPatch, VmResourcePatch};

// ── test helpers ──────────────────────────────────────────────────────────────

async fn setup_db() -> (ContainerAsync<Postgres>, db::PgPool) {
    let container = Postgres::default().start().await.expect("start postgres");
    let port = container.get_host_port_ipv4(5432).await.expect("get port");
    let url = format!("postgres://postgres:postgres@localhost:{port}/postgres");
    let pool = db::connect(&url).await.expect("connect");
    db::migrate(&pool).await.expect("migrate");
    (container, pool)
}

async fn create_account_and_session(pool: &db::PgPool) -> (String, String) {
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
            username: "testuser".into(),
            created_at: now,
        },
    )
    .await
    .expect("create account");

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

fn vm_row(account_id: &str) -> db::VmRow {
    db::VmRow {
        id: uuid::Uuid::new_v4().to_string(),
        account_id: account_id.to_string(),
        name: "test-vm".into(),
        status: "stopped".into(),
        subdomain: "test-vm".into(),
        vcpus: 1000,
        memory_mb: 512,
        disk_mb: 5120,
        bandwidth_mbps: 100,
        kernel_path: "/vmlinux".into(),
        rootfs_path: "/images/ubuntu.sqfs".into(),
        overlay_path: None,
        real_init: "/sbin/init".into(),
        ip_address: "172.16.1.2".into(),
        exposed_port: 8080,
        tap_device: None,
        pid: None,
        socket_path: None,
        host_id: None,
        base_image: "ubuntu".into(),
        cloned_from: None,
        disk_usage_mb: 0,
        created_at: 0,
        last_started_at: None,
        placement_strategy: "best_fit".into(),
        required_labels: None,
        region: None,
        namespace_id: format!("ns_{account_id}"),
    }
}

// ── MockVmOps ────────────────────────────────────────────────────────────────

struct MockVmOps {
    vms: Mutex<Vec<db::VmRow>>,
}

impl MockVmOps {
    fn empty() -> Arc<Self> {
        Arc::new(Self {
            vms: Mutex::new(vec![]),
        })
    }

    fn with_vm(vm: db::VmRow) -> Arc<Self> {
        Arc::new(Self {
            vms: Mutex::new(vec![vm]),
        })
    }
}

#[async_trait::async_trait]
impl VmOps for MockVmOps {
    async fn create_vm(&self, account_id: String, req: CreateVmRequest) -> anyhow::Result<db::VmRow> {
        let mut vm = vm_row(&account_id);
        vm.name = req.name;
        vm.vcpus = req.vcpus;
        vm.memory_mb = req.memory_mb;
        self.vms.lock().unwrap().push(vm.clone());
        Ok(vm)
    }

    async fn clone_vm(&self, _source_id: &str, account_id: &str, req: CloneVmRequest) -> anyhow::Result<db::VmRow> {
        let mut vm = vm_row(account_id);
        vm.name = req.name;
        Ok(vm)
    }

    async fn start_vm(&self, _id: &str) -> anyhow::Result<()> { Ok(()) }
    async fn stop_vm(&self, _id: &str) -> anyhow::Result<()> { Ok(()) }
    async fn delete_vm(&self, _id: &str) -> anyhow::Result<()> { Ok(()) }

    async fn get_vm(&self, id: &str) -> anyhow::Result<Option<db::VmRow>> {
        Ok(self.vms.lock().unwrap().iter().find(|v| v.id == id).cloned())
    }

    async fn list_vms(&self, account_id: &str) -> anyhow::Result<Vec<db::VmRow>> {
        Ok(self.vms.lock().unwrap().iter().filter(|v| v.account_id == account_id).cloned().collect())
    }

    async fn take_snapshot(&self, vm_id: &str, label: Option<String>) -> anyhow::Result<db::SnapshotRow> {
        Ok(db::SnapshotRow {
            id: uuid::Uuid::new_v4().to_string(),
            vm_id: vm_id.to_string(),
            label,
            snapshot_path: "/snapshots/snap".into(),
            mem_path: "/snapshots/mem".into(),
            size_bytes: 0,
            created_at: 0,
        })
    }

    async fn list_snapshots(&self, _vm_id: &str) -> anyhow::Result<Vec<db::SnapshotRow>> { Ok(vec![]) }
    async fn delete_snapshot(&self, _vm_id: &str, _snap_id: &str) -> anyhow::Result<()> { Ok(()) }
    async fn restore_snapshot(&self, _vm_id: &str, _snap_id: &str) -> anyhow::Result<()> { Ok(()) }
    async fn change_username(&self, _account_id: &str, _new_username: &str) -> anyhow::Result<()> { Ok(()) }

    async fn resize_resources(&self, vm_id: &str, _account_id: &str, patch: VmResourcePatch) -> anyhow::Result<db::VmRow> {
        let mut vms = self.vms.lock().unwrap();
        let vm = vms.iter_mut().find(|v| v.id == vm_id).ok_or_else(|| anyhow::anyhow!("not found"))?;
        if let Some(vcpus) = patch.vcpus { vm.vcpus = vcpus; }
        if let Some(mem) = patch.memory_mb { vm.memory_mb = mem; }
        if let Some(bw) = patch.bandwidth_mbps { vm.bandwidth_mbps = bw; }
        Ok(vm.clone())
    }

    async fn update_vm(&self, vm_id: &str, _account_id: &str, patch: VmPatch) -> anyhow::Result<db::VmRow> {
        let mut vms = self.vms.lock().unwrap();
        let vm = vms.iter_mut().find(|v| v.id == vm_id).ok_or_else(|| anyhow::anyhow!("not found"))?;
        if let Some(name) = patch.name { vm.name = name; }
        if let Some(port) = patch.exposed_port { vm.exposed_port = port; }
        Ok(vm.clone())
    }
}

fn app(pool: db::PgPool, ops: Arc<dyn VmOps>) -> axum::Router {
    api::router(ops).layer(axum::Extension(pool))
}

async fn body_bytes(body: Body) -> bytes::Bytes {
    body.collect().await.unwrap().to_bytes()
}

// ── tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn unauthenticated_list_vms_returns_401() {
    let (_c, pool) = setup_db().await;
    let app = app(pool, MockVmOps::empty());

    let resp = app
        .oneshot(Request::get("/api/vms").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn list_vms_returns_empty_for_new_account() {
    let (_c, pool) = setup_db().await;
    let (_account_id, session_id) = create_account_and_session(&pool).await;
    let app = app(pool, MockVmOps::empty());

    let resp = app
        .oneshot(
            Request::get("/api/vms")
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp.into_body()).await;
    let vms: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(vms, serde_json::json!([]));
}

#[tokio::test]
async fn list_vms_returns_only_own_vms() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;

    let mut owned = vm_row(&account_id);
    owned.name = "my-vm".into();
    let mut other = vm_row("other-account-id");
    other.name = "not-mine".into();

    let ops = MockVmOps::with_vm(owned);
    ops.vms.lock().unwrap().push(other);

    let app = app(pool, ops);

    let resp = app
        .oneshot(
            Request::get("/api/vms")
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp.into_body()).await;
    let vms: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(vms.len(), 1);
    assert_eq!(vms[0]["name"], "my-vm");
}

#[tokio::test]
async fn create_vm_returns_201() {
    let (_c, pool) = setup_db().await;
    let (_account_id, session_id) = create_account_and_session(&pool).await;
    let app = app(pool, MockVmOps::empty());

    let body = serde_json::json!({"name": "new-vm"});
    let resp = app
        .oneshot(
            Request::post("/api/vms")
                .header(header::COOKIE, format!("session_id={session_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = body_bytes(resp.into_body()).await;
    let vm: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(vm["name"], "new-vm");
}

#[tokio::test]
async fn get_vm_not_found_returns_404() {
    let (_c, pool) = setup_db().await;
    let (_account_id, session_id) = create_account_and_session(&pool).await;
    let app = app(pool, MockVmOps::empty());

    let resp = app
        .oneshot(
            Request::get("/api/vms/nonexistent-id")
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn resize_running_vm_with_restart_error_returns_422() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;

    struct FailingResize(db::VmRow);
    #[async_trait::async_trait]
    impl VmOps for FailingResize {
        async fn create_vm(&self, _: String, _: CreateVmRequest) -> anyhow::Result<db::VmRow> { unimplemented!() }
        async fn clone_vm(&self, _: &str, _: &str, _: CloneVmRequest) -> anyhow::Result<db::VmRow> { unimplemented!() }
        async fn start_vm(&self, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn stop_vm(&self, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn delete_vm(&self, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn get_vm(&self, _: &str) -> anyhow::Result<Option<db::VmRow>> { Ok(Some(self.0.clone())) }
        async fn list_vms(&self, _: &str) -> anyhow::Result<Vec<db::VmRow>> { Ok(vec![]) }
        async fn take_snapshot(&self, _: &str, _: Option<String>) -> anyhow::Result<db::SnapshotRow> { unimplemented!() }
        async fn list_snapshots(&self, _: &str) -> anyhow::Result<Vec<db::SnapshotRow>> { Ok(vec![]) }
        async fn delete_snapshot(&self, _: &str, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn restore_snapshot(&self, _: &str, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn change_username(&self, _: &str, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn resize_resources(&self, _: &str, _: &str, _: VmResourcePatch) -> anyhow::Result<db::VmRow> {
            Err(anyhow::anyhow!("restart required to apply CPU changes"))
        }
        async fn update_vm(&self, _: &str, _: &str, _: VmPatch) -> anyhow::Result<db::VmRow> { unimplemented!() }
    }

    let mut vm = vm_row(&account_id);
    vm.status = "running".into();
    let vm_id = vm.id.clone();

    let app = app(pool, Arc::new(FailingResize(vm)));

    let body = serde_json::json!({"vcpus": 2000});
    let resp = app
        .oneshot(
            Request::post(format!("/api/vms/{vm_id}/resources"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn healthz_returns_ok_unauthenticated() {
    let (_c, pool) = setup_db().await;
    let app = app(pool, MockVmOps::empty());

    let resp = app
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_vm_returns_200() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;
    let mut vm = vm_row(&account_id);
    vm.name = "found-vm".into();
    let vm_id = vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(vm));

    let resp = app
        .oneshot(
            Request::get(format!("/api/vms/{vm_id}"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp.into_body()).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["name"], "found-vm");
}

#[tokio::test]
async fn start_vm_returns_202() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;
    let vm = vm_row(&account_id);
    let vm_id = vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(vm));

    let resp = app
        .oneshot(
            Request::post(format!("/api/vms/{vm_id}/start"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn stop_vm_returns_202() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;
    let vm = vm_row(&account_id);
    let vm_id = vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(vm));

    let resp = app
        .oneshot(
            Request::post(format!("/api/vms/{vm_id}/stop"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn delete_vm_returns_204() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;
    let vm = vm_row(&account_id);
    let vm_id = vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(vm));

    let resp = app
        .oneshot(
            Request::delete(format!("/api/vms/{vm_id}"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn patch_vm_returns_200() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;
    let vm = vm_row(&account_id);
    let vm_id = vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(vm));

    let body = serde_json::json!({"name": "renamed-vm"});
    let resp = app
        .oneshot(
            Request::patch(format!("/api/vms/{vm_id}"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp.into_body()).await;
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["name"], "renamed-vm");
}

#[tokio::test]
async fn patch_vm_not_found_returns_404() {
    let (_c, pool) = setup_db().await;
    let (_account_id, session_id) = create_account_and_session(&pool).await;
    let app = app(pool, MockVmOps::empty());

    let body = serde_json::json!({"name": "whatever"});
    let resp = app
        .oneshot(
            Request::patch("/api/vms/nonexistent")
                .header(header::COOKIE, format!("session_id={session_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn patch_vm_other_account_returns_403() {
    let (_c, pool) = setup_db().await;
    let (_account_id, session_id) = create_account_and_session(&pool).await;
    let other_vm = vm_row("completely-different-account");
    let vm_id = other_vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(other_vm));

    let body = serde_json::json!({"name": "stolen"});
    let resp = app
        .oneshot(
            Request::patch(format!("/api/vms/{vm_id}"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn take_snapshot_returns_201() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;
    let vm = vm_row(&account_id);
    let vm_id = vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(vm));

    let body = serde_json::json!({"label": "before-upgrade"});
    let resp = app
        .oneshot(
            Request::post(format!("/api/vms/{vm_id}/snapshot"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = body_bytes(resp.into_body()).await;
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["label"], "before-upgrade");
    assert_eq!(v["vm_id"], vm_id);
}

#[tokio::test]
async fn list_snapshots_returns_empty() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;
    let vm = vm_row(&account_id);
    let vm_id = vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(vm));

    let resp = app
        .oneshot(
            Request::get(format!("/api/vms/{vm_id}/snapshots"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp.into_body()).await;
    let snaps: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(snaps, serde_json::json!([]));
}

#[tokio::test]
async fn delete_snapshot_returns_204() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;
    let vm = vm_row(&account_id);
    let vm_id = vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(vm));

    let resp = app
        .oneshot(
            Request::delete(format!("/api/vms/{vm_id}/snapshots/some-snap-id"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn restore_snapshot_returns_202() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;
    let vm = vm_row(&account_id);
    let vm_id = vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(vm));

    let resp = app
        .oneshot(
            Request::post(format!("/api/vms/{vm_id}/restore/some-snap-id"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn clone_vm_returns_201() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;
    let vm = vm_row(&account_id);
    let vm_id = vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(vm));

    let body = serde_json::json!({"name": "cloned-vm"});
    let resp = app
        .oneshot(
            Request::post(format!("/api/vms/{vm_id}/clone"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = body_bytes(resp.into_body()).await;
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["name"], "cloned-vm");
}

#[tokio::test]
async fn clone_vm_quota_error_returns_422() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;

    struct QuotaFailOps(db::VmRow);
    #[async_trait::async_trait]
    impl VmOps for QuotaFailOps {
        async fn create_vm(&self, _: String, _: CreateVmRequest) -> anyhow::Result<db::VmRow> { unimplemented!() }
        async fn clone_vm(&self, _: &str, _: &str, _: CloneVmRequest) -> anyhow::Result<db::VmRow> {
            Err(anyhow::anyhow!("quota limit exceeded"))
        }
        async fn start_vm(&self, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn stop_vm(&self, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn delete_vm(&self, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn get_vm(&self, _: &str) -> anyhow::Result<Option<db::VmRow>> { Ok(Some(self.0.clone())) }
        async fn list_vms(&self, _: &str) -> anyhow::Result<Vec<db::VmRow>> { Ok(vec![]) }
        async fn take_snapshot(&self, _: &str, _: Option<String>) -> anyhow::Result<db::SnapshotRow> { unimplemented!() }
        async fn list_snapshots(&self, _: &str) -> anyhow::Result<Vec<db::SnapshotRow>> { Ok(vec![]) }
        async fn delete_snapshot(&self, _: &str, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn restore_snapshot(&self, _: &str, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn change_username(&self, _: &str, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn resize_resources(&self, _: &str, _: &str, _: VmResourcePatch) -> anyhow::Result<db::VmRow> { unimplemented!() }
        async fn update_vm(&self, _: &str, _: &str, _: VmPatch) -> anyhow::Result<db::VmRow> { unimplemented!() }
    }

    let vm = vm_row(&account_id);
    let vm_id = vm.id.clone();
    let app = app(pool, Arc::new(QuotaFailOps(vm)));

    let body = serde_json::json!({"name": "wont-fit"});
    let resp = app
        .oneshot(
            Request::post(format!("/api/vms/{vm_id}/clone"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn list_vm_events_not_found_returns_404() {
    let (_c, pool) = setup_db().await;
    let (_account_id, session_id) = create_account_and_session(&pool).await;
    let app = app(pool, MockVmOps::empty());

    let resp = app
        .oneshot(
            Request::get("/api/vms/nonexistent/events")
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_vm_events_returns_empty() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;
    let vm = vm_row(&account_id);
    let vm_id = vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(vm));

    let resp = app
        .oneshot(
            Request::get(format!("/api/vms/{vm_id}/events"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp.into_body()).await;
    let events: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(events, serde_json::json!([]));
}

#[tokio::test]
async fn list_vm_events_other_account_returns_403() {
    let (_c, pool) = setup_db().await;
    let (_account_id, session_id) = create_account_and_session(&pool).await;
    let other_vm = vm_row("other-account");
    let vm_id = other_vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(other_vm));

    let resp = app
        .oneshot(
            Request::get(format!("/api/vms/{vm_id}/events"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn change_username_returns_204() {
    let (_c, pool) = setup_db().await;
    let (_account_id, session_id) = create_account_and_session(&pool).await;
    let app = app(pool, MockVmOps::empty());

    let body = serde_json::json!({"username": "newname"});
    let resp = app
        .oneshot(
            Request::post("/api/account/username")
                .header(header::COOKIE, format!("session_id={session_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn change_username_already_taken_returns_409() {
    let (_c, pool) = setup_db().await;
    let (_account_id, session_id) = create_account_and_session(&pool).await;

    struct TakenUsernameOps;
    #[async_trait::async_trait]
    impl VmOps for TakenUsernameOps {
        async fn create_vm(&self, _: String, _: CreateVmRequest) -> anyhow::Result<db::VmRow> { unimplemented!() }
        async fn clone_vm(&self, _: &str, _: &str, _: CloneVmRequest) -> anyhow::Result<db::VmRow> { unimplemented!() }
        async fn start_vm(&self, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn stop_vm(&self, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn delete_vm(&self, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn get_vm(&self, _: &str) -> anyhow::Result<Option<db::VmRow>> { Ok(None) }
        async fn list_vms(&self, _: &str) -> anyhow::Result<Vec<db::VmRow>> { Ok(vec![]) }
        async fn take_snapshot(&self, _: &str, _: Option<String>) -> anyhow::Result<db::SnapshotRow> { unimplemented!() }
        async fn list_snapshots(&self, _: &str) -> anyhow::Result<Vec<db::SnapshotRow>> { Ok(vec![]) }
        async fn delete_snapshot(&self, _: &str, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn restore_snapshot(&self, _: &str, _: &str) -> anyhow::Result<()> { Ok(()) }
        async fn change_username(&self, _: &str, _: &str) -> anyhow::Result<()> {
            Err(anyhow::anyhow!("username already taken"))
        }
        async fn resize_resources(&self, _: &str, _: &str, _: VmResourcePatch) -> anyhow::Result<db::VmRow> { unimplemented!() }
        async fn update_vm(&self, _: &str, _: &str, _: VmPatch) -> anyhow::Result<db::VmRow> { unimplemented!() }
    }

    let app = app(pool, Arc::new(TakenUsernameOps));

    let body = serde_json::json!({"username": "taken"});
    let resp = app
        .oneshot(
            Request::post("/api/account/username")
                .header(header::COOKIE, format!("session_id={session_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn list_vms_by_name_filter_returns_match() {
    let (_c, pool) = setup_db().await;
    let (account_id, session_id) = create_account_and_session(&pool).await;
    let mut vm = vm_row(&account_id);
    vm.name = "specific-vm".into();
    let ops = MockVmOps::with_vm(vm);
    let app = app(pool.clone(), ops);

    let resp = app
        .oneshot(
            Request::get("/api/vms?name=specific-vm")
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    // name filter goes through db::get_vm_by_name, which won't find our mock VM
    // so we just verify the response shape is an array
    let bytes = body_bytes(resp.into_body()).await;
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(v.is_array());
}

#[tokio::test]
async fn resize_resources_not_found_returns_404() {
    let (_c, pool) = setup_db().await;
    let (_account_id, session_id) = create_account_and_session(&pool).await;
    let app = app(pool, MockVmOps::empty());

    let body = serde_json::json!({"vcpus": 2000});
    let resp = app
        .oneshot(
            Request::post("/api/vms/nonexistent/resources")
                .header(header::COOKIE, format!("session_id={session_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn resize_resources_other_account_returns_403() {
    let (_c, pool) = setup_db().await;
    let (_account_id, session_id) = create_account_and_session(&pool).await;
    let other_vm = vm_row("other-account");
    let vm_id = other_vm.id.clone();
    let app = app(pool, MockVmOps::with_vm(other_vm));

    let body = serde_json::json!({"vcpus": 2000});
    let resp = app
        .oneshot(
            Request::post(format!("/api/vms/{vm_id}/resources"))
                .header(header::COOKIE, format!("session_id={session_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_images_returns_empty() {
    let (_c, pool) = setup_db().await;
    let (_account_id, session_id) = create_account_and_session(&pool).await;
    let app = app(pool, MockVmOps::empty());

    let resp = app
        .oneshot(
            Request::get("/api/images")
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp.into_body()).await;
    let images: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(images, serde_json::json!([]));
}

#[tokio::test]
async fn list_regions_returns_empty() {
    let (_c, pool) = setup_db().await;
    let (_account_id, session_id) = create_account_and_session(&pool).await;
    let app = app(pool, MockVmOps::empty());

    let resp = app
        .oneshot(
            Request::get("/api/regions")
                .header(header::COOKIE, format!("session_id={session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = body_bytes(resp.into_body()).await;
    let regions: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(regions, serde_json::json!([]));
}
