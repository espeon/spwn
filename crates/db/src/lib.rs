use sqlx::Row;
use thiserror::Error;

pub use sqlx::PgPool;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migrate error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

pub type Result<T> = std::result::Result<T, DbError>;

#[derive(Debug, Clone)]
pub struct VmRow {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub status: String,
    pub subdomain: String,
    pub vcores: i32,
    pub memory_mb: i32,
    pub kernel_path: String,
    pub rootfs_path: String,
    pub overlay_path: Option<String>,
    pub real_init: String,
    pub ip_address: String,
    pub exposed_port: i32,
    pub tap_device: Option<String>,
    pub pid: Option<i64>,
    pub socket_path: Option<String>,
    pub host_id: Option<String>,
    pub created_at: i64,
    pub last_started_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct HostRow {
    pub id: String,
    pub name: String,
    pub address: String,
    pub vcpu_total: i32,
    pub mem_total_mb: i32,
    pub images_dir: String,
    pub overlay_dir: String,
    pub snapshot_dir: String,
    pub kernel_path: String,
    pub last_seen_at: i64,
}

pub struct NewHost {
    pub id: String,
    pub name: String,
    pub address: String,
    pub vcpu_total: i32,
    pub mem_total_mb: i32,
    pub images_dir: String,
    pub overlay_dir: String,
    pub snapshot_dir: String,
    pub kernel_path: String,
}

pub struct NewVm {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub subdomain: String,
    pub vcores: i32,
    pub memory_mb: i32,
    pub kernel_path: String,
    pub rootfs_path: String,
    pub overlay_path: String,
    pub real_init: String,
    pub ip_address: String,
    pub exposed_port: i32,
}

#[derive(Debug, Clone)]
pub struct SnapshotRow {
    pub id: String,
    pub vm_id: String,
    pub label: Option<String>,
    pub snapshot_path: String,
    pub mem_path: String,
    pub size_bytes: i64,
    pub created_at: i64,
}

pub struct NewSnapshot {
    pub id: String,
    pub vm_id: String,
    pub label: Option<String>,
    pub snapshot_path: String,
    pub mem_path: String,
    pub size_bytes: i64,
}

pub async fn connect(database_url: &str) -> Result<PgPool> {
    Ok(PgPool::connect(database_url).await?)
}

pub async fn migrate(pool: &PgPool) -> Result<()> {
    sqlx::migrate!().run(pool).await?;
    Ok(())
}

pub async fn create_vm(pool: &PgPool, vm: &NewVm) -> Result<()> {
    let now = unix_now();
    sqlx::query(
        "INSERT INTO vms (id, account_id, name, status, subdomain, vcores, memory_mb,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, created_at)
         VALUES ($1,$2,$3,'stopped',$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(&vm.id)
    .bind(&vm.account_id)
    .bind(&vm.name)
    .bind(&vm.subdomain)
    .bind(vm.vcores)
    .bind(vm.memory_mb)
    .bind(&vm.kernel_path)
    .bind(&vm.rootfs_path)
    .bind(&vm.overlay_path)
    .bind(&vm.real_init)
    .bind(&vm.ip_address)
    .bind(vm.exposed_port)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_vm(pool: &PgPool, id: &str) -> Result<Option<VmRow>> {
    let row = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcores, memory_mb,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, created_at, last_started_at FROM vms WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_vm))
}

pub async fn list_vms(pool: &PgPool, account_id: &str) -> Result<Vec<VmRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcores, memory_mb,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, created_at, last_started_at FROM vms WHERE account_id = $1
         ORDER BY created_at DESC",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_vm).collect())
}

pub async fn get_vms_by_status(pool: &PgPool, status: &str) -> Result<Vec<VmRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcores, memory_mb,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, created_at, last_started_at FROM vms WHERE status = $1",
    )
    .bind(status)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_vm).collect())
}

pub async fn get_all_vms(pool: &PgPool) -> Result<Vec<VmRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcores, memory_mb,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, created_at, last_started_at FROM vms",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_vm).collect())
}

pub async fn get_vms_by_host(pool: &PgPool, host_id: &str) -> Result<Vec<VmRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcores, memory_mb,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, created_at, last_started_at FROM vms WHERE host_id = $1",
    )
    .bind(host_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_vm).collect())
}

pub async fn set_vm_host(pool: &PgPool, vm_id: &str, host_id: &str) -> Result<()> {
    sqlx::query("UPDATE vms SET host_id = $1 WHERE id = $2")
        .bind(host_id)
        .bind(vm_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_vm_status(pool: &PgPool, id: &str, status: &str) -> Result<()> {
    sqlx::query("UPDATE vms SET status = $1 WHERE id = $2")
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_vm_running(pool: &PgPool, id: &str, pid: i64, tap_device: &str, socket_path: &str) -> Result<()> {
    let now = unix_now();
    sqlx::query(
        "UPDATE vms SET status='running', pid=$1, tap_device=$2, socket_path=$3, last_started_at=$4 WHERE id=$5",
    )
    .bind(pid)
    .bind(tap_device)
    .bind(socket_path)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_vm_stopped(pool: &PgPool, id: &str) -> Result<()> {
    sqlx::query(
        "UPDATE vms SET status='stopped', pid=NULL, tap_device=NULL, socket_path=NULL WHERE id=$1",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_vm(pool: &PgPool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM vms WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_used_ips(pool: &PgPool) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT ip_address FROM vms")
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(|r| r.get::<String, _>("ip_address")).collect())
}

pub async fn log_event(pool: &PgPool, vm_id: &str, event: &str, metadata: Option<&str>) -> Result<()> {
    let now = unix_now();
    sqlx::query(
        "INSERT INTO vm_events (vm_id, event, metadata, created_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(vm_id)
    .bind(event)
    .bind(metadata)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

fn row_to_vm(r: sqlx::postgres::PgRow) -> VmRow {
    VmRow {
        id: r.get("id"),
        account_id: r.get("account_id"),
        name: r.get("name"),
        status: r.get("status"),
        subdomain: r.get("subdomain"),
        vcores: r.get("vcores"),
        memory_mb: r.get("memory_mb"),
        kernel_path: r.get("kernel_path"),
        rootfs_path: r.get("rootfs_path"),
        overlay_path: r.get("overlay_path"),
        real_init: r.get("real_init"),
        ip_address: r.get("ip_address"),
        exposed_port: r.get("exposed_port"),
        tap_device: r.get("tap_device"),
        pid: r.get("pid"),
        socket_path: r.get("socket_path"),
        host_id: r.get("host_id"),
        created_at: r.get("created_at"),
        last_started_at: r.get("last_started_at"),
    }
}

pub async fn upsert_host(pool: &PgPool, host: &NewHost) -> Result<HostRow> {
    let now = unix_now();
    sqlx::query(
        "INSERT INTO hosts (id, name, address, vcpu_total, mem_total_mb, images_dir, overlay_dir, snapshot_dir, kernel_path, last_seen_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
         ON CONFLICT (id) DO UPDATE SET
           name=$2, address=$3, vcpu_total=$4, mem_total_mb=$5,
           images_dir=$6, overlay_dir=$7, snapshot_dir=$8, kernel_path=$9, last_seen_at=$10",
    )
    .bind(&host.id)
    .bind(&host.name)
    .bind(&host.address)
    .bind(host.vcpu_total)
    .bind(host.mem_total_mb)
    .bind(&host.images_dir)
    .bind(&host.overlay_dir)
    .bind(&host.snapshot_dir)
    .bind(&host.kernel_path)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(HostRow {
        id: host.id.clone(),
        name: host.name.clone(),
        address: host.address.clone(),
        vcpu_total: host.vcpu_total,
        mem_total_mb: host.mem_total_mb,
        images_dir: host.images_dir.clone(),
        overlay_dir: host.overlay_dir.clone(),
        snapshot_dir: host.snapshot_dir.clone(),
        kernel_path: host.kernel_path.clone(),
        last_seen_at: now,
    })
}

pub async fn update_host_heartbeat(
    pool: &PgPool,
    host_id: &str,
    now: i64,
) -> Result<()> {
    sqlx::query("UPDATE hosts SET last_seen_at=$1 WHERE id=$2")
        .bind(now)
        .bind(host_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_hosts(pool: &PgPool) -> Result<Vec<HostRow>> {
    let rows = sqlx::query(
        "SELECT id, name, address, vcpu_total, mem_total_mb, images_dir, overlay_dir,
         snapshot_dir, kernel_path, last_seen_at FROM hosts",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_host).collect())
}

pub async fn get_host(pool: &PgPool, id: &str) -> Result<Option<HostRow>> {
    let row = sqlx::query(
        "SELECT id, name, address, vcpu_total, mem_total_mb, images_dir, overlay_dir,
         snapshot_dir, kernel_path, last_seen_at FROM hosts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_host))
}

fn row_to_host(r: sqlx::postgres::PgRow) -> HostRow {
    HostRow {
        id: r.get("id"),
        name: r.get("name"),
        address: r.get("address"),
        vcpu_total: r.get("vcpu_total"),
        mem_total_mb: r.get("mem_total_mb"),
        images_dir: r.get("images_dir"),
        overlay_dir: r.get("overlay_dir"),
        snapshot_dir: r.get("snapshot_dir"),
        kernel_path: r.get("kernel_path"),
        last_seen_at: r.get("last_seen_at"),
    }
}

pub async fn create_snapshot(pool: &PgPool, snap: &NewSnapshot) -> Result<SnapshotRow> {
    let now = unix_now();
    sqlx::query(
        "INSERT INTO snapshots (id, vm_id, label, snapshot_path, mem_path, size_bytes, created_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(&snap.id)
    .bind(&snap.vm_id)
    .bind(&snap.label)
    .bind(&snap.snapshot_path)
    .bind(&snap.mem_path)
    .bind(snap.size_bytes)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(SnapshotRow {
        id: snap.id.clone(),
        vm_id: snap.vm_id.clone(),
        label: snap.label.clone(),
        snapshot_path: snap.snapshot_path.clone(),
        mem_path: snap.mem_path.clone(),
        size_bytes: snap.size_bytes,
        created_at: now,
    })
}

pub async fn get_snapshot(pool: &PgPool, id: &str) -> Result<Option<SnapshotRow>> {
    let row = sqlx::query(
        "SELECT id, vm_id, label, snapshot_path, mem_path, size_bytes, created_at
         FROM snapshots WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_snapshot))
}

pub async fn list_snapshots(pool: &PgPool, vm_id: &str) -> Result<Vec<SnapshotRow>> {
    let rows = sqlx::query(
        "SELECT id, vm_id, label, snapshot_path, mem_path, size_bytes, created_at
         FROM snapshots WHERE vm_id = $1 ORDER BY created_at DESC",
    )
    .bind(vm_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_snapshot).collect())
}

pub async fn count_snapshots(pool: &PgPool, vm_id: &str) -> Result<i64> {
    let row = sqlx::query("SELECT COUNT(*) as n FROM snapshots WHERE vm_id = $1")
        .bind(vm_id)
        .fetch_one(pool)
        .await?;
    Ok(row.get("n"))
}

pub async fn delete_snapshot(pool: &PgPool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM snapshots WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

fn row_to_snapshot(r: sqlx::postgres::PgRow) -> SnapshotRow {
    SnapshotRow {
        id: r.get("id"),
        vm_id: r.get("vm_id"),
        label: r.get("label"),
        snapshot_path: r.get("snapshot_path"),
        mem_path: r.get("mem_path"),
        size_bytes: r.get("size_bytes"),
        created_at: r.get("created_at"),
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
