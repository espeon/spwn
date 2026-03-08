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
    pub ip_address: String,
    pub exposed_port: i32,
    pub tap_device: Option<String>,
    pub pid: Option<i64>,
    pub socket_path: Option<String>,
    pub created_at: i64,
    pub last_started_at: Option<i64>,
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
    pub ip_address: String,
    pub exposed_port: i32,
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
         kernel_path, rootfs_path, ip_address, exposed_port, created_at)
         VALUES ($1,$2,$3,'stopped',$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(&vm.id)
    .bind(&vm.account_id)
    .bind(&vm.name)
    .bind(&vm.subdomain)
    .bind(vm.vcores)
    .bind(vm.memory_mb)
    .bind(&vm.kernel_path)
    .bind(&vm.rootfs_path)
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
         kernel_path, rootfs_path, ip_address, exposed_port, tap_device, pid,
         socket_path, created_at, last_started_at FROM vms WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_vm))
}

pub async fn list_vms(pool: &PgPool, account_id: &str) -> Result<Vec<VmRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcores, memory_mb,
         kernel_path, rootfs_path, ip_address, exposed_port, tap_device, pid,
         socket_path, created_at, last_started_at FROM vms WHERE account_id = $1
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
         kernel_path, rootfs_path, ip_address, exposed_port, tap_device, pid,
         socket_path, created_at, last_started_at FROM vms WHERE status = $1",
    )
    .bind(status)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_vm).collect())
}

pub async fn get_all_vms(pool: &PgPool) -> Result<Vec<VmRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcores, memory_mb,
         kernel_path, rootfs_path, ip_address, exposed_port, tap_device, pid,
         socket_path, created_at, last_started_at FROM vms",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_vm).collect())
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
        ip_address: r.get("ip_address"),
        exposed_port: r.get("exposed_port"),
        tap_device: r.get("tap_device"),
        pid: r.get("pid"),
        socket_path: r.get("socket_path"),
        created_at: r.get("created_at"),
        last_started_at: r.get("last_started_at"),
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
