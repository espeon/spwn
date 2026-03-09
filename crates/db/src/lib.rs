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
    pub vcpus: f64,
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
    pub vcpus: f64,
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
        "INSERT INTO vms (id, account_id, name, status, subdomain, vcpus, memory_mb,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, created_at)
         VALUES ($1,$2,$3,'stopped',$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(&vm.id)
    .bind(&vm.account_id)
    .bind(&vm.name)
    .bind(&vm.subdomain)
    .bind(vm.vcpus)
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
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb,
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
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb,
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
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb,
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
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, created_at, last_started_at FROM vms",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_vm).collect())
}

pub async fn get_vms_by_host(pool: &PgPool, host_id: &str) -> Result<Vec<VmRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb,
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

pub async fn set_vm_running(
    pool: &PgPool,
    id: &str,
    pid: i64,
    tap_device: &str,
    socket_path: &str,
) -> Result<()> {
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

pub async fn set_vm_pid(pool: &PgPool, id: &str, pid: i64) -> Result<()> {
    sqlx::query("UPDATE vms SET pid=$1 WHERE id=$2")
        .bind(pid)
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
    Ok(rows
        .iter()
        .map(|r| r.get::<String, _>("ip_address"))
        .collect())
}

pub async fn log_event(
    pool: &PgPool,
    vm_id: &str,
    event: &str,
    metadata: Option<&str>,
) -> Result<()> {
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
        vcpus: r.get("vcpus"),
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

pub async fn update_host_heartbeat(pool: &PgPool, host_id: &str, now: i64) -> Result<()> {
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

// ── accounts ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AccountRow {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_bytes: Option<Vec<u8>>,
    pub theme: String,
    pub vcpu_limit: i32,
    pub mem_limit_mb: i32,
    pub vm_limit: i32,
    pub created_at: i64,
}

pub struct NewAccount {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub username: String,
    pub created_at: i64,
}

pub struct UpdateTheme {
    pub theme: String,
}

pub struct UpdateAccountProfile {
    pub display_name: Option<String>,
    pub avatar_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: String,
    pub account_id: String,
    pub created_at: i64,
    pub expires_at: i64,
}

pub struct NewSession {
    pub id: String,
    pub account_id: String,
    pub created_at: i64,
    pub expires_at: i64,
}

pub async fn create_account(pool: &PgPool, account: &NewAccount) -> Result<AccountRow> {
    sqlx::query(
        "INSERT INTO accounts (id, email, password_hash, username, created_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&account.id)
    .bind(&account.email)
    .bind(&account.password_hash)
    .bind(&account.username)
    .bind(account.created_at)
    .execute(pool)
    .await?;
    Ok(AccountRow {
        id: account.id.clone(),
        email: account.email.clone(),
        password_hash: account.password_hash.clone(),
        username: account.username.clone(),
        display_name: None,
        avatar_bytes: None,
        theme: "catppuccin-latte".into(),
        vcpu_limit: 8,
        mem_limit_mb: 12288,
        vm_limit: 5,
        created_at: account.created_at,
    })
}

pub async fn get_account_by_email(pool: &PgPool, email: &str) -> Result<Option<AccountRow>> {
    let row = sqlx::query(
        "SELECT id, email, password_hash, username, display_name, avatar_bytes, theme,
         vcpu_limit, mem_limit_mb, vm_limit, created_at
         FROM accounts WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_account))
}

pub async fn get_account_by_username(pool: &PgPool, username: &str) -> Result<Option<AccountRow>> {
    let row = sqlx::query(
        "SELECT id, email, password_hash, username, display_name, avatar_bytes, theme,
         vcpu_limit, mem_limit_mb, vm_limit, created_at
         FROM accounts WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_account))
}

pub async fn get_account(pool: &PgPool, id: &str) -> Result<Option<AccountRow>> {
    let row = sqlx::query(
        "SELECT id, email, password_hash, username, display_name, avatar_bytes, theme,
         vcpu_limit, mem_limit_mb, vm_limit, created_at
         FROM accounts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_account))
}

pub struct UsernameUpdate {
    pub old_username: String,
    pub new_username: String,
}

#[derive(Debug, Clone)]
pub struct RenamedSubdomain {
    pub vm_id: String,
    pub old_subdomain: String,
    pub new_subdomain: String,
}

pub async fn update_username(
    pool: &PgPool,
    account_id: &str,
    update: &UsernameUpdate,
) -> Result<Vec<RenamedSubdomain>> {
    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE accounts SET username = $1 WHERE id = $2")
        .bind(&update.new_username)
        .bind(account_id)
        .execute(&mut *tx)
        .await?;

    let vms = sqlx::query("SELECT id, subdomain FROM vms WHERE account_id = $1")
        .bind(account_id)
        .fetch_all(&mut *tx)
        .await?;

    let mut renamed = Vec::with_capacity(vms.len());

    for vm in &vms {
        let vm_id: String = vm.get("id");
        let old_subdomain: String = vm.get("subdomain");

        let new_subdomain = if let Some(vm_name) =
            old_subdomain.strip_suffix(&format!(".{}", update.old_username))
        {
            format!("{vm_name}.{}", update.new_username)
        } else {
            old_subdomain.clone()
        };

        if new_subdomain != old_subdomain {
            sqlx::query("UPDATE vms SET subdomain = $1 WHERE id = $2")
                .bind(&new_subdomain)
                .bind(&vm_id)
                .execute(&mut *tx)
                .await?;

            renamed.push(RenamedSubdomain {
                vm_id,
                old_subdomain,
                new_subdomain,
            });
        }
    }

    tx.commit().await?;
    Ok(renamed)
}

pub async fn update_theme(pool: &PgPool, id: &str, update: &UpdateTheme) -> Result<()> {
    sqlx::query("UPDATE accounts SET theme = $1 WHERE id = $2")
        .bind(&update.theme)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_account_profile(
    pool: &PgPool,
    id: &str,
    update: &UpdateAccountProfile,
) -> Result<()> {
    sqlx::query("UPDATE accounts SET display_name = $1, avatar_bytes = $2 WHERE id = $3")
        .bind(&update.display_name)
        .bind(&update.avatar_bytes)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

fn row_to_account(r: sqlx::postgres::PgRow) -> AccountRow {
    AccountRow {
        id: r.get("id"),
        email: r.get("email"),
        password_hash: r.get("password_hash"),
        username: r.get("username"),
        display_name: r.get("display_name"),
        avatar_bytes: r.get("avatar_bytes"),
        theme: r.get("theme"),
        vcpu_limit: r.get("vcpu_limit"),
        mem_limit_mb: r.get("mem_limit_mb"),
        vm_limit: r.get("vm_limit"),
        created_at: r.get("created_at"),
    }
}

// ── sessions ──────────────────────────────────────────────────────────────────

pub async fn create_session(pool: &PgPool, session: &NewSession) -> Result<SessionRow> {
    sqlx::query(
        "INSERT INTO sessions (id, account_id, created_at, expires_at)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(&session.id)
    .bind(&session.account_id)
    .bind(session.created_at)
    .bind(session.expires_at)
    .execute(pool)
    .await?;
    Ok(SessionRow {
        id: session.id.clone(),
        account_id: session.account_id.clone(),
        created_at: session.created_at,
        expires_at: session.expires_at,
    })
}

pub async fn get_session(pool: &PgPool, id: &str) -> Result<Option<SessionRow>> {
    let row =
        sqlx::query("SELECT id, account_id, created_at, expires_at FROM sessions WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| SessionRow {
        id: r.get("id"),
        account_id: r.get("account_id"),
        created_at: r.get("created_at"),
        expires_at: r.get("expires_at"),
    }))
}

pub async fn delete_session(pool: &PgPool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_expired_sessions(pool: &PgPool) -> Result<u64> {
    let now = unix_now();
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at < $1")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ── quota ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum QuotaError {
    Exceeded(String),
    Db(DbError),
    Serialization,
}

impl std::fmt::Display for QuotaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuotaError::Exceeded(msg) => write!(f, "quota exceeded: {msg}"),
            QuotaError::Db(e) => write!(f, "db error: {e}"),
            QuotaError::Serialization => write!(f, "serialization conflict, retry"),
        }
    }
}

impl std::error::Error for QuotaError {}

impl From<DbError> for QuotaError {
    fn from(e: DbError) -> Self {
        QuotaError::Db(e)
    }
}

/// Check account quota and atomically set the VM to status='starting'.
/// Runs in a SERIALIZABLE transaction to prevent concurrent starts from racing.
/// Returns Err(QuotaError::Serialization) on conflict — caller should retry once.
pub async fn check_quota_and_reserve(
    pool: &PgPool,
    account_id: &str,
    vm_id: &str,
    vcpus: f64,
    mem_mb: i32,
) -> std::result::Result<(), QuotaError> {
    let mut tx = pool.begin().await.map_err(DbError::from)?;

    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *tx)
        .await
        .map_err(DbError::from)?;

    let account_row =
        sqlx::query("SELECT vcpu_limit, mem_limit_mb, vm_limit FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(DbError::from)?
            .ok_or_else(|| QuotaError::Exceeded("account not found".into()))?;

    let vcpu_limit: f64 = account_row.get::<i32, _>("vcpu_limit") as f64;
    let mem_limit: i32 = account_row.get("mem_limit_mb");
    let vm_limit: i32 = account_row.get("vm_limit");

    let usage_row = sqlx::query(
        "SELECT COALESCE(SUM(vcpus),0)::float8 AS used_vcpus,
                COALESCE(SUM(memory_mb),0)::int AS used_mem,
                COUNT(*)::int AS used_vms
         FROM vms
         WHERE account_id = $1 AND status IN ('running','starting')",
    )
    .bind(account_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(DbError::from)?;

    let used_vcpus: f64 = usage_row.get("used_vcpus");
    let used_mem: i32 = usage_row.get("used_mem");
    let used_vms: i32 = usage_row.get("used_vms");

    if used_vms >= vm_limit {
        return Err(QuotaError::Exceeded(format!(
            "vm limit {vm_limit} reached ({used_vms} running/starting)"
        )));
    }
    if used_vcpus + vcpus > vcpu_limit {
        return Err(QuotaError::Exceeded(format!(
            "vcpu limit {vcpu_limit} would be exceeded ({used_vcpus} used + {vcpus} requested)"
        )));
    }
    if used_mem + mem_mb > mem_limit {
        return Err(QuotaError::Exceeded(format!(
            "memory limit {mem_limit}MB would be exceeded ({used_mem} used + {mem_mb} requested)"
        )));
    }

    sqlx::query("UPDATE vms SET status='starting' WHERE id = $1")
        .bind(vm_id)
        .execute(&mut *tx)
        .await
        .map_err(DbError::from)?;

    tx.commit().await.map_err(|e| {
        // sqlx wraps postgres errors; check for serialization failure (40001)
        let msg = e.to_string();
        if msg.contains("40001") || msg.contains("could not serialize") {
            QuotaError::Serialization
        } else {
            QuotaError::Db(DbError::Sqlx(e))
        }
    })?;

    Ok(())
}

// ── vm events ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VmEventRow {
    pub id: i64,
    pub vm_id: String,
    pub event: String,
    pub metadata: Option<String>,
    pub created_at: i64,
}

pub async fn get_vm_by_name(pool: &PgPool, account_id: &str, name: &str) -> Result<Option<VmRow>> {
    let row = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, created_at, last_started_at
         FROM vms WHERE account_id = $1 AND name = $2",
    )
    .bind(account_id)
    .bind(name)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_vm))
}

pub async fn list_vm_events(
    pool: &PgPool,
    vm_id: &str,
    limit: i64,
    before: Option<i64>,
) -> Result<Vec<VmEventRow>> {
    let rows = if let Some(cursor) = before {
        sqlx::query(
            "SELECT id, vm_id, event, metadata, created_at FROM vm_events
             WHERE vm_id = $1 AND id < $2 ORDER BY id DESC LIMIT $3",
        )
        .bind(vm_id)
        .bind(cursor)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT id, vm_id, event, metadata, created_at FROM vm_events
             WHERE vm_id = $1 ORDER BY id DESC LIMIT $2",
        )
        .bind(vm_id)
        .bind(limit)
        .fetch_all(pool)
        .await?
    };
    Ok(rows
        .into_iter()
        .map(|r| VmEventRow {
            id: r.get("id"),
            vm_id: r.get("vm_id"),
            event: r.get("event"),
            metadata: r.get("metadata"),
            created_at: r.get("created_at"),
        })
        .collect())
}

pub async fn rename_vm(
    pool: &PgPool,
    vm_id: &str,
    new_name: &str,
    new_subdomain: &str,
) -> Result<()> {
    sqlx::query("UPDATE vms SET name = $1, subdomain = $2 WHERE id = $3")
        .bind(new_name)
        .bind(new_subdomain)
        .bind(vm_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_vm_port(pool: &PgPool, vm_id: &str, exposed_port: i32) -> Result<()> {
    sqlx::query("UPDATE vms SET exposed_port = $1 WHERE id = $2")
        .bind(exposed_port)
        .bind(vm_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── CLI auth codes ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CliAuthCode {
    pub code: String,
    pub account_id: Option<String>,
    pub status: String,
    pub expires_at: i64,
}

pub async fn create_cli_auth_code(pool: &PgPool, code: &str, expires_at: i64) -> Result<()> {
    sqlx::query("INSERT INTO cli_auth_codes (code, status, expires_at) VALUES ($1, 'pending', $2)")
        .bind(code)
        .bind(expires_at)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_cli_auth_code(pool: &PgPool, code: &str) -> Result<Option<CliAuthCode>> {
    let row = sqlx::query(
        "SELECT code, account_id, status, expires_at FROM cli_auth_codes WHERE code = $1",
    )
    .bind(code)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| CliAuthCode {
        code: r.get("code"),
        account_id: r.get("account_id"),
        status: r.get("status"),
        expires_at: r.get("expires_at"),
    }))
}

pub async fn authorize_cli_auth_code(pool: &PgPool, code: &str, account_id: &str) -> Result<()> {
    sqlx::query("UPDATE cli_auth_codes SET status = 'authorized', account_id = $1 WHERE code = $2")
        .bind(account_id)
        .bind(code)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn deny_cli_auth_code(pool: &PgPool, code: &str) -> Result<()> {
    sqlx::query("UPDATE cli_auth_codes SET status = 'denied' WHERE code = $1")
        .bind(code)
        .execute(pool)
        .await?;
    Ok(())
}

// ── API tokens ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ApiTokenRow {
    pub id: String,
    pub account_id: String,
    pub token_hash: String,
    pub name: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

pub struct NewApiToken {
    pub id: String,
    pub account_id: String,
    pub token_hash: String,
    pub name: String,
}

pub async fn create_api_token(pool: &PgPool, token: &NewApiToken) -> Result<ApiTokenRow> {
    let now = unix_now();
    sqlx::query(
        "INSERT INTO api_tokens (id, account_id, token_hash, name, created_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&token.id)
    .bind(&token.account_id)
    .bind(&token.token_hash)
    .bind(&token.name)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(ApiTokenRow {
        id: token.id.clone(),
        account_id: token.account_id.clone(),
        token_hash: token.token_hash.clone(),
        name: token.name.clone(),
        created_at: now,
        last_used_at: None,
    })
}

pub async fn get_account_id_by_token_hash(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<String>> {
    let row = sqlx::query("SELECT account_id FROM api_tokens WHERE token_hash = $1")
        .bind(token_hash)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get("account_id")))
}

pub async fn touch_api_token(pool: &PgPool, token_hash: &str, now: i64) -> Result<()> {
    sqlx::query("UPDATE api_tokens SET last_used_at = $1 WHERE token_hash = $2")
        .bind(now)
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
