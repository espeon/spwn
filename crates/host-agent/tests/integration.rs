//! Integration tests for host-agent VM lifecycle.
//!
//! These tests require a real host with KVM, firecracker, and jailer installed,
//! plus the spwn postgres database running. They are marked #[ignore] and must
//! be run explicitly on a prepared host:
//!
//!   sudo -E cargo test -p host-agent --test integration -- --ignored
//!
//! Required env vars (same as the agent itself):
//!   FIRECRACKER_BIN, JAILER_BIN, KERNEL_PATH, IMAGES_DIR, DATABASE_URL
//!   JAILER_CHROOT_BASE, EXTERNAL_IFACE
//!   JAILER_UID, JAILER_GID (optional — falls back to spwn-vm user/group)

use std::{path::PathBuf, sync::Arc};

use fctools::vmm::installation::VmmInstallation;
use networking::NetworkManager;

// ── helpers ───────────────────────────────────────────────────────────────────

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("{key} must be set for integration tests"))
}

fn env_path(key: &str) -> PathBuf {
    PathBuf::from(env(key))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn resolve_id_from_file(path: &str, name: &str) -> anyhow::Result<u32> {
    let contents =
        std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("read {path}: {e}"))?;
    for line in contents.lines() {
        let mut fields = line.splitn(4, ':');
        let entry_name = match fields.next() {
            Some(n) => n,
            None => continue,
        };
        if entry_name != name {
            continue;
        }
        let _password = fields.next();
        let id_str = match fields.next() {
            Some(s) => s,
            None => continue,
        };
        if let Ok(id) = id_str.parse::<u32>() {
            return Ok(id);
        }
    }
    Err(anyhow::anyhow!("'{name}' not found in {path}"))
}

fn resolve_jailer_uid() -> u32 {
    if let Ok(val) = std::env::var("JAILER_UID") {
        return val.parse::<u32>().expect("JAILER_UID must be a valid u32");
    }
    resolve_id_from_file("/etc/passwd", "spwn-vm")
        .expect("spwn-vm user not found in /etc/passwd — create it or set JAILER_UID")
}

fn resolve_jailer_gid() -> u32 {
    if let Ok(val) = std::env::var("JAILER_GID") {
        return val.parse::<u32>().expect("JAILER_GID must be a valid u32");
    }
    resolve_id_from_file("/etc/group", "spwn-vm")
        .expect("spwn-vm group not found in /etc/group — create it or set JAILER_GID")
}

async fn setup() -> (db::PgPool, Arc<host_agent::manager::VmManager>) {
    dotenvy::dotenv().ok();

    let database_url = env_or("DATABASE_URL", "postgres://postgres:spwn@localhost/spwn");
    let pool = db::connect(&database_url).await.expect("connect to db");
    db::migrate(&pool).await.expect("run migrations");

    let installation = VmmInstallation::new(
        env_path("FIRECRACKER_BIN"),
        env_path("JAILER_BIN"),
        env_or("SNAPSHOT_EDITOR_BIN", "/usr/local/bin/snapshot-editor").into(),
    );

    let jailer_uid = resolve_jailer_uid();
    let jailer_gid = resolve_jailer_gid();
    let chroot_base_dir = std::path::PathBuf::from(env_or("JAILER_CHROOT_BASE", "/srv/jailer"));

    let manager = Arc::new(host_agent::manager::VmManager::new(
        pool.clone(),
        NetworkManager::new(),
        installation,
        env_path("KERNEL_PATH"),
        env_path("IMAGES_DIR").into(),
        env_or("OVERLAY_DIR", "/var/lib/spwn/overlays").into(),
        env_or("SNAPSHOT_DIR", "/var/lib/spwn/snapshots").into(),
        "test-host".to_string(),
        jailer_uid,
        jailer_gid,
        chroot_base_dir,
    ));

    (pool, manager)
}

/// Insert a minimal account + VM row directly, bypassing the API layer.
async fn insert_test_vm(pool: &db::PgPool, vcpus: i64, memory_mb: i32) -> (String, String) {
    let account_id = uuid::Uuid::new_v4().to_string();
    let vm_id = uuid::Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO accounts (id, email, password_hash, username, theme,
         vcpu_limit, mem_limit_mb, vm_limit, created_at)
         VALUES ($1,$2,'hash',$2,'catppuccin-latte',8000,12288,5,0)",
    )
    .bind(&account_id)
    .bind(format!("test-{}@example.com", &account_id[..8]))
    .execute(pool)
    .await
    .expect("insert account");

    let images_dir = std::env::var("IMAGES_DIR").unwrap_or_else(|_| "/var/lib/spwn/images".into());
    let overlay_dir =
        std::env::var("OVERLAY_DIR").unwrap_or_else(|_| "/var/lib/spwn/overlays".into());
    let rootfs = format!("{images_dir}/default.sqfs");
    let overlay = format!("{overlay_dir}/{vm_id}.ext4");
    let kernel = std::env::var("KERNEL_PATH").unwrap_or_else(|_| "/tmp/vmlinux".into());

    sqlx::query(
        "INSERT INTO vms (id, account_id, name, status, subdomain, vcpus, memory_mb,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, created_at)
         VALUES ($1,$2,'test-vm','stopped',$1,$3,$4,$5,$6,$7,'/sbin/init','172.16.1.2',8080,0)",
    )
    .bind(&vm_id)
    .bind(&account_id)
    .bind(vcpus)
    .bind(memory_mb)
    .bind(&kernel)
    .bind(&rootfs)
    .bind(&overlay)
    .execute(pool)
    .await
    .expect("insert vm");

    (account_id, vm_id)
}

async fn cleanup_vm(pool: &db::PgPool, vm_id: &str, account_id: &str) {
    sqlx::query("DELETE FROM vms WHERE id = $1")
        .bind(vm_id)
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(account_id)
        .execute(pool)
        .await
        .ok();
}

// ── vm lifecycle ──────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires KVM + firecracker + jailer on host"]
async fn test_start_vm_creates_jailed_process() {
    let (pool, manager) = setup().await;
    let (account_id, vm_id) = insert_test_vm(&pool, 1000, 256).await;

    manager.start_vm(&vm_id).await.expect("start vm");

    let vm = db::get_vm(&pool, &vm_id)
        .await
        .expect("query")
        .expect("should exist");

    assert_eq!(vm.status, "running", "vm should be running after start");
    assert!(vm.pid.is_some(), "pid should be recorded");
    assert!(vm.tap_device.is_some(), "tap device should be assigned");

    // cgroup should exist at the deterministic jailer path
    let cgroup_path = format!("/sys/fs/cgroup/firecracker/{}/cgroup.procs", vm_id);
    assert!(
        std::path::Path::new(&cgroup_path).exists(),
        "jailer cgroup should exist at {cgroup_path}"
    );

    manager.stop_vm(&vm_id).await.expect("stop vm");
    cleanup_vm(&pool, &vm_id, &account_id).await;
}

#[tokio::test]
#[ignore = "requires KVM + firecracker + jailer on host"]
async fn test_stop_vm_releases_tap_and_clears_pid() {
    let (pool, manager) = setup().await;
    let (account_id, vm_id) = insert_test_vm(&pool, 1000, 256).await;

    manager.start_vm(&vm_id).await.expect("start vm");
    manager.stop_vm(&vm_id).await.expect("stop vm");

    let vm = db::get_vm(&pool, &vm_id)
        .await
        .expect("query")
        .expect("should exist");

    assert_eq!(vm.status, "stopped");
    assert!(vm.pid.is_none(), "pid should be cleared after stop");
    assert!(vm.tap_device.is_none(), "tap should be released after stop");

    cleanup_vm(&pool, &vm_id, &account_id).await;
}

#[tokio::test]
#[ignore = "requires KVM + firecracker + jailer on host"]
async fn test_stop_already_stopped_vm_is_idempotent() {
    let (pool, manager) = setup().await;
    let (account_id, vm_id) = insert_test_vm(&pool, 1000, 256).await;

    // should not error on a vm that was never started
    manager
        .stop_vm(&vm_id)
        .await
        .expect("stop on stopped vm should be a no-op");

    cleanup_vm(&pool, &vm_id, &account_id).await;
}

#[tokio::test]
#[ignore = "requires KVM + firecracker + jailer on host"]
async fn test_take_and_restore_snapshot() {
    let (pool, manager) = setup().await;
    let (account_id, vm_id) = insert_test_vm(&pool, 1000, 256).await;

    manager.start_vm(&vm_id).await.expect("start vm");

    let snap = manager
        .take_snapshot(&vm_id, Some("test-snap".to_string()))
        .await
        .expect("take snapshot");

    assert!(!snap.id.is_empty());
    assert!(
        std::path::Path::new(&snap.snapshot_path).exists(),
        "snapshot file should exist on host at {}",
        snap.snapshot_path
    );
    assert!(
        std::path::Path::new(&snap.mem_path).exists(),
        "mem file should exist on host at {}",
        snap.mem_path
    );

    manager.stop_vm(&vm_id).await.expect("stop before restore");
    manager
        .restore_snapshot(&vm_id, &snap.id)
        .await
        .expect("restore snapshot");

    let vm = db::get_vm(&pool, &vm_id)
        .await
        .expect("query")
        .expect("should exist");
    assert_eq!(vm.status, "running", "vm should be running after restore");

    manager.stop_vm(&vm_id).await.expect("stop restored vm");
    cleanup_vm(&pool, &vm_id, &account_id).await;
}

#[tokio::test]
#[ignore = "requires KVM + firecracker + jailer on host"]
async fn test_snapshot_limit_enforced() {
    let (pool, manager) = setup().await;
    let (account_id, vm_id) = insert_test_vm(&pool, 1000, 256).await;

    manager.start_vm(&vm_id).await.expect("start vm");

    manager
        .take_snapshot(&vm_id, Some("snap-1".to_string()))
        .await
        .expect("first snapshot");
    manager
        .take_snapshot(&vm_id, Some("snap-2".to_string()))
        .await
        .expect("second snapshot");

    let err = manager
        .take_snapshot(&vm_id, Some("snap-3".to_string()))
        .await
        .expect_err("third snapshot should fail — limit is 2");
    assert!(
        err.to_string().contains("limit"),
        "error should mention snapshot limit, got: {err}"
    );

    manager.stop_vm(&vm_id).await.expect("stop vm");
    cleanup_vm(&pool, &vm_id, &account_id).await;
}

#[tokio::test]
#[ignore = "requires KVM + firecracker + jailer on host"]
async fn test_delete_vm_removes_overlay() {
    let (pool, manager) = setup().await;
    let (account_id, vm_id) = insert_test_vm(&pool, 1000, 256).await;

    manager.start_vm(&vm_id).await.expect("start vm");
    manager.stop_vm(&vm_id).await.expect("stop vm");

    let vm = db::get_vm(&pool, &vm_id)
        .await
        .expect("query")
        .expect("should exist");
    let overlay_path = vm.overlay_path.clone().expect("overlay path should be set");

    manager.delete_vm(&vm_id).await.expect("delete vm");

    assert!(
        !std::path::Path::new(&overlay_path).exists(),
        "overlay file should be removed after delete"
    );
    assert!(
        db::get_vm(&pool, &vm_id).await.expect("query").is_none(),
        "vm row should be gone after delete"
    );

    cleanup_vm(&pool, &vm_id, &account_id).await;
}
