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
    pub vcpus: i64,
    pub memory_mb: i32,
    pub disk_mb: i32,
    pub bandwidth_mbps: i32,
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
    pub base_image: String,
    pub cloned_from: Option<String>,
    pub disk_usage_mb: i32,
    pub created_at: i64,
    pub last_started_at: Option<i64>,
    pub placement_strategy: String,
    pub required_labels: Option<serde_json::Value>,
    pub region: Option<String>,
    pub namespace_id: String,
}

#[derive(Debug, Clone)]
pub struct NamespaceRow {
    pub id: String,
    pub slug: String,
    pub kind: String,
    pub display_name: Option<String>,
    pub owner_id: String,
    pub vcpu_limit: i64,
    pub mem_limit_mb: i32,
    pub vm_limit: i32,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct MemberRow {
    pub namespace_id: String,
    pub account_id: String,
    pub username: String,
    pub role: String,
    pub joined_at: i64,
}

pub struct NewNamespace {
    pub id: String,
    pub slug: String,
    pub kind: String,
    pub display_name: Option<String>,
    pub owner_id: String,
    pub vcpu_limit: i64,
    pub mem_limit_mb: i32,
    pub vm_limit: i32,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct HostRow {
    pub id: String,
    pub name: String,
    pub address: String,
    pub vcpu_total: i64,
    pub mem_total_mb: i32,
    pub images_dir: String,
    pub overlay_dir: String,
    pub snapshot_dir: String,
    pub kernel_path: String,
    pub last_seen_at: i64,
    pub status: String,
    pub vcpu_used: i64,
    pub mem_used_mb: i32,
    pub labels: serde_json::Value,
    pub snapshot_addr: String,
}

pub struct NewHost {
    pub id: String,
    pub name: String,
    pub address: String,
    pub vcpu_total: i64,
    pub mem_total_mb: i32,
    pub images_dir: String,
    pub overlay_dir: String,
    pub snapshot_dir: String,
    pub kernel_path: String,
    pub snapshot_addr: String,
}

pub struct NewVm {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub subdomain: String,
    pub vcpus: i64,
    pub memory_mb: i32,
    pub disk_mb: i32,
    pub bandwidth_mbps: i32,
    pub kernel_path: String,
    pub rootfs_path: String,
    pub overlay_path: String,
    pub real_init: String,
    pub ip_address: String,
    pub exposed_port: i32,
    pub base_image: String,
    pub cloned_from: Option<String>,
    pub placement_strategy: String,
    pub required_labels: Option<serde_json::Value>,
    pub region: Option<String>,
    pub namespace_id: String,
}

#[derive(Debug, Clone)]
pub struct AdminVmRecord {
    pub id: String,
    pub name: String,
    pub status: String,
    pub host_id: Option<String>,
    pub account_id: String,
    pub username: String,
    pub vcpus: i64,
    pub memory_mb: i32,
    pub disk_usage_mb: i32,
    pub subdomain: String,
    pub region: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RegionInfo {
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct VmMigrationRow {
    pub id: String,
    pub vm_id: String,
    pub from_host: String,
    pub to_host: String,
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

pub struct NewVmMigration {
    pub id: String,
    pub vm_id: String,
    pub from_host: String,
    pub to_host: String,
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
         disk_mb, bandwidth_mbps, kernel_path, rootfs_path, overlay_path, real_init,
         ip_address, exposed_port, base_image, cloned_from, placement_strategy,
         required_labels, region, namespace_id, created_at)
         VALUES ($1,$2,$3,'stopped',$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)",
    )
    .bind(&vm.id)
    .bind(&vm.account_id)
    .bind(&vm.name)
    .bind(&vm.subdomain)
    .bind(vm.vcpus)
    .bind(vm.memory_mb)
    .bind(vm.disk_mb)
    .bind(vm.bandwidth_mbps)
    .bind(&vm.kernel_path)
    .bind(&vm.rootfs_path)
    .bind(&vm.overlay_path)
    .bind(&vm.real_init)
    .bind(&vm.ip_address)
    .bind(vm.exposed_port)
    .bind(&vm.base_image)
    .bind(&vm.cloned_from)
    .bind(&vm.placement_strategy)
    .bind(vm.required_labels.as_ref().map(sqlx::types::Json))
    .bind(&vm.region)
    .bind(&vm.namespace_id)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_vm(pool: &PgPool, id: &str) -> Result<Option<VmRow>> {
    let row = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb, disk_mb, bandwidth_mbps,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, base_image, cloned_from, disk_usage_mb, created_at, last_started_at,
         placement_strategy, required_labels, region, namespace_id FROM vms WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_vm))
}

pub async fn get_vm_by_subdomain(pool: &PgPool, subdomain: &str) -> Result<Option<VmRow>> {
    let row = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb, disk_mb, bandwidth_mbps,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, base_image, cloned_from, disk_usage_mb, created_at, last_started_at,
         placement_strategy, required_labels, region, namespace_id FROM vms WHERE subdomain = $1",
    )
    .bind(subdomain)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_vm))
}

pub async fn list_vms(pool: &PgPool, account_id: &str) -> Result<Vec<VmRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb, disk_mb, bandwidth_mbps,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, base_image, cloned_from, disk_usage_mb, created_at, last_started_at,
         placement_strategy, required_labels, region, namespace_id FROM vms WHERE account_id = $1
         ORDER BY created_at DESC",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_vm).collect())
}

pub async fn list_vms_by_namespace(pool: &PgPool, namespace_id: &str) -> Result<Vec<VmRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb, disk_mb, bandwidth_mbps,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, base_image, cloned_from, disk_usage_mb, created_at, last_started_at,
         placement_strategy, required_labels, region, namespace_id FROM vms WHERE namespace_id = $1
         ORDER BY created_at DESC",
    )
    .bind(namespace_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_vm).collect())
}

pub async fn list_all_vms_admin(pool: &PgPool) -> Result<Vec<AdminVmRecord>> {
    let rows = sqlx::query(
        "SELECT v.id, v.name, v.status, v.host_id, v.account_id, a.username,
         v.vcpus, v.memory_mb, v.disk_usage_mb, v.subdomain, v.region
         FROM vms v
         JOIN accounts a ON v.account_id = a.id
         ORDER BY v.host_id NULLS LAST, v.name",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| AdminVmRecord {
            id: r.get("id"),
            name: r.get("name"),
            status: r.get("status"),
            host_id: r.get("host_id"),
            account_id: r.get("account_id"),
            username: r.get("username"),
            vcpus: r.get("vcpus"),
            memory_mb: r.get("memory_mb"),
            disk_usage_mb: r.get("disk_usage_mb"),
            subdomain: r.get("subdomain"),
            region: r.get("region"),
        })
        .collect())
}

pub async fn get_vms_by_status(pool: &PgPool, status: &str) -> Result<Vec<VmRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb, disk_mb, bandwidth_mbps,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, base_image, cloned_from, disk_usage_mb, created_at, last_started_at,
         placement_strategy, required_labels, region, namespace_id FROM vms WHERE status = $1",
    )
    .bind(status)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_vm).collect())
}

pub async fn get_all_vms(pool: &PgPool) -> Result<Vec<VmRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb, disk_mb, bandwidth_mbps,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, base_image, cloned_from, disk_usage_mb, created_at, last_started_at,
         placement_strategy, required_labels, region, namespace_id FROM vms",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_vm).collect())
}

pub async fn get_vms_by_host(pool: &PgPool, host_id: &str) -> Result<Vec<VmRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb, disk_mb, bandwidth_mbps,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, base_image, cloned_from, disk_usage_mb, created_at, last_started_at,
         placement_strategy, required_labels, region, namespace_id FROM vms WHERE host_id = $1",
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

pub async fn get_used_ips_for_host(pool: &PgPool, host_id: &str) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT ip_address FROM vms WHERE host_id = $1")
        .bind(host_id)
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
        disk_mb: r.get("disk_mb"),
        bandwidth_mbps: r.get("bandwidth_mbps"),
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
        base_image: r.get("base_image"),
        cloned_from: r.get("cloned_from"),
        disk_usage_mb: r.get("disk_usage_mb"),
        created_at: r.get("created_at"),
        last_started_at: r.get("last_started_at"),
        placement_strategy: r.get("placement_strategy"),
        required_labels: r.get("required_labels"),
        region: r.get("region"),
        namespace_id: r.get("namespace_id"),
    }
}

pub async fn set_vm_region(pool: &PgPool, vm_id: &str, region: &str) -> Result<()> {
    sqlx::query("UPDATE vms SET region = $1 WHERE id = $2")
        .bind(region)
        .bind(vm_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_regions(pool: &PgPool) -> Result<Vec<RegionInfo>> {
    let rows = sqlx::query(
        "SELECT
             labels->>'region' AS name,
             BOOL_OR(status = 'active') AS active
         FROM hosts
         WHERE labels->>'region' IS NOT NULL
         GROUP BY labels->>'region'
         ORDER BY labels->>'region'",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| RegionInfo {
            name: r.get("name"),
            active: r.get("active"),
        })
        .collect())
}

pub async fn update_disk_usage_mb(pool: &PgPool, vm_id: &str, disk_usage_mb: i32) -> Result<()> {
    sqlx::query("UPDATE vms SET disk_usage_mb = $1 WHERE id = $2")
        .bind(disk_usage_mb)
        .bind(vm_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn upsert_host(pool: &PgPool, host: &NewHost) -> Result<HostRow> {
    let now = unix_now();
    sqlx::query(
        "INSERT INTO hosts (id, name, address, vcpu_total, mem_total_mb, images_dir, overlay_dir,
           snapshot_dir, kernel_path, snapshot_addr, last_seen_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
         ON CONFLICT (id) DO UPDATE SET
           name=$2, address=$3, vcpu_total=$4, mem_total_mb=$5,
           images_dir=$6, overlay_dir=$7, snapshot_dir=$8, kernel_path=$9,
           snapshot_addr=$10, last_seen_at=$11",
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
    .bind(&host.snapshot_addr)
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
        status: "active".into(),
        vcpu_used: 0,
        mem_used_mb: 0,
        labels: serde_json::Value::Object(Default::default()),
        snapshot_addr: host.snapshot_addr.clone(),
    })
}

pub async fn update_host_heartbeat(
    pool: &PgPool,
    host_id: &str,
    vcpu_used: i64,
    mem_used_mb: i32,
) -> Result<()> {
    let now = unix_now();
    sqlx::query(
        "UPDATE hosts SET
            last_seen_at = $1,
            vcpu_used    = $2,
            mem_used_mb  = $3,
            status = CASE WHEN status = 'draining' THEN 'draining' ELSE 'active' END
         WHERE id = $4",
    )
    .bind(now)
    .bind(vcpu_used)
    .bind(mem_used_mb)
    .bind(host_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_hosts(pool: &PgPool) -> Result<Vec<HostRow>> {
    let rows = sqlx::query(
        "SELECT id, name, address, vcpu_total, mem_total_mb, images_dir, overlay_dir,
         snapshot_dir, kernel_path, last_seen_at, status, vcpu_used, mem_used_mb, labels, snapshot_addr FROM hosts",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_host).collect())
}

pub async fn get_host(pool: &PgPool, id: &str) -> Result<Option<HostRow>> {
    let row = sqlx::query(
        "SELECT id, name, address, vcpu_total, mem_total_mb, images_dir, overlay_dir,
         snapshot_dir, kernel_path, last_seen_at, status, vcpu_used, mem_used_mb, labels, snapshot_addr FROM hosts WHERE id = $1",
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
        status: r.get("status"),
        vcpu_used: r.get("vcpu_used"),
        mem_used_mb: r.get("mem_used_mb"),
        labels: r.get("labels"),
        snapshot_addr: r.get("snapshot_addr"),
    }
}

pub async fn set_host_status(pool: &PgPool, host_id: &str, status: &str) -> Result<()> {
    sqlx::query("UPDATE hosts SET status = $1 WHERE id = $2")
        .bind(status)
        .bind(host_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_active_hosts(pool: &PgPool) -> Result<Vec<HostRow>> {
    let rows = sqlx::query(
        "SELECT id, name, address, vcpu_total, mem_total_mb, images_dir, overlay_dir,
         snapshot_dir, kernel_path, last_seen_at, status, vcpu_used, mem_used_mb, labels, snapshot_addr
         FROM hosts WHERE status = 'active'",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_host).collect())
}

pub async fn create_vm_migration(pool: &PgPool, m: &NewVmMigration) -> Result<VmMigrationRow> {
    let now = unix_now();
    sqlx::query(
        "INSERT INTO vm_migrations (id, vm_id, from_host, to_host, status, started_at)
         VALUES ($1, $2, $3, $4, 'pending', $5)",
    )
    .bind(&m.id)
    .bind(&m.vm_id)
    .bind(&m.from_host)
    .bind(&m.to_host)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(VmMigrationRow {
        id: m.id.clone(),
        vm_id: m.vm_id.clone(),
        from_host: m.from_host.clone(),
        to_host: m.to_host.clone(),
        status: "pending".into(),
        started_at: now,
        finished_at: None,
    })
}

pub async fn update_migration_status(
    pool: &PgPool,
    id: &str,
    status: &str,
    finished_at: Option<i64>,
) -> Result<()> {
    sqlx::query("UPDATE vm_migrations SET status = $1, finished_at = $2 WHERE id = $3")
        .bind(status)
        .bind(finished_at)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_vm_migrations(pool: &PgPool, vm_id: &str) -> Result<Vec<VmMigrationRow>> {
    let rows = sqlx::query(
        "SELECT id, vm_id, from_host, to_host, status, started_at, finished_at
         FROM vm_migrations WHERE vm_id = $1 ORDER BY started_at DESC",
    )
    .bind(vm_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| VmMigrationRow {
            id: r.get("id"),
            vm_id: r.get("vm_id"),
            from_host: r.get("from_host"),
            to_host: r.get("to_host"),
            status: r.get("status"),
            started_at: r.get("started_at"),
            finished_at: r.get("finished_at"),
        })
        .collect())
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
    pub vcpu_limit: i64,
    pub mem_limit_mb: i32,
    pub vm_limit: i32,
    pub created_at: i64,
    pub role: String,
    pub dotfiles_repo: Option<String>,
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
    pub dotfiles_repo: Option<String>,
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
        vcpu_limit: 8000,
        mem_limit_mb: 12288,
        vm_limit: 5,
        created_at: account.created_at,
        role: "user".into(),
        dotfiles_repo: None,
    })
}

pub async fn get_account_by_email(pool: &PgPool, email: &str) -> Result<Option<AccountRow>> {
    let row = sqlx::query(
        "SELECT id, email, password_hash, username, display_name, avatar_bytes, theme,
         vcpu_limit, mem_limit_mb, vm_limit, created_at, role, dotfiles_repo
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
         vcpu_limit, mem_limit_mb, vm_limit, created_at, role, dotfiles_repo
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
         vcpu_limit, mem_limit_mb, vm_limit, created_at, role, dotfiles_repo
         FROM accounts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_account))
}

pub struct UsernameUpdate {
    pub new_username: String,
}

pub async fn update_username(
    pool: &PgPool,
    account_id: &str,
    update: &UsernameUpdate,
) -> Result<()> {
    sqlx::query("UPDATE accounts SET username = $1 WHERE id = $2")
        .bind(&update.new_username)
        .bind(account_id)
        .execute(pool)
        .await?;
    Ok(())
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
    sqlx::query(
        "UPDATE accounts SET display_name = $1, avatar_bytes = $2, dotfiles_repo = $3 WHERE id = $4",
    )
    .bind(&update.display_name)
    .bind(&update.avatar_bytes)
    .bind(&update.dotfiles_repo)
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
        role: r.get("role"),
        dotfiles_repo: r.get("dotfiles_repo"),
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

// ── namespaces ────────────────────────────────────────────────────────────────

fn row_to_namespace(r: sqlx::postgres::PgRow) -> NamespaceRow {
    NamespaceRow {
        id: r.get("id"),
        slug: r.get("slug"),
        kind: r.get("kind"),
        display_name: r.get("display_name"),
        owner_id: r.get("owner_id"),
        vcpu_limit: r.get("vcpu_limit"),
        mem_limit_mb: r.get("mem_limit_mb"),
        vm_limit: r.get("vm_limit"),
        created_at: r.get("created_at"),
    }
}

pub async fn create_namespace(pool: &PgPool, ns: &NewNamespace) -> Result<NamespaceRow> {
    sqlx::query(
        "INSERT INTO namespaces (id, slug, kind, display_name, owner_id, vcpu_limit, mem_limit_mb, vm_limit, created_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(&ns.id)
    .bind(&ns.slug)
    .bind(&ns.kind)
    .bind(&ns.display_name)
    .bind(&ns.owner_id)
    .bind(ns.vcpu_limit)
    .bind(ns.mem_limit_mb)
    .bind(ns.vm_limit)
    .bind(ns.created_at)
    .execute(pool)
    .await?;
    Ok(NamespaceRow {
        id: ns.id.clone(),
        slug: ns.slug.clone(),
        kind: ns.kind.clone(),
        display_name: ns.display_name.clone(),
        owner_id: ns.owner_id.clone(),
        vcpu_limit: ns.vcpu_limit,
        mem_limit_mb: ns.mem_limit_mb,
        vm_limit: ns.vm_limit,
        created_at: ns.created_at,
    })
}

pub async fn add_namespace_member(
    pool: &PgPool,
    namespace_id: &str,
    account_id: &str,
    role: &str,
) -> Result<()> {
    let now = unix_now();
    sqlx::query(
        "INSERT INTO namespace_members (namespace_id, account_id, role, joined_at)
         VALUES ($1,$2,$3,$4)
         ON CONFLICT (namespace_id, account_id) DO NOTHING",
    )
    .bind(namespace_id)
    .bind(account_id)
    .bind(role)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_namespace(pool: &PgPool, id: &str) -> Result<Option<NamespaceRow>> {
    let row = sqlx::query(
        "SELECT id, slug, kind, display_name, owner_id, vcpu_limit, mem_limit_mb, vm_limit, created_at
         FROM namespaces WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_namespace))
}

pub async fn get_personal_namespace(pool: &PgPool, account_id: &str) -> Result<Option<NamespaceRow>> {
    let row = sqlx::query(
        "SELECT id, slug, kind, display_name, owner_id, vcpu_limit, mem_limit_mb, vm_limit, created_at
         FROM namespaces WHERE owner_id = $1 AND kind = 'personal'",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_namespace))
}

pub async fn list_namespaces_for_account(pool: &PgPool, account_id: &str) -> Result<Vec<NamespaceRow>> {
    let rows = sqlx::query(
        "SELECT n.id, n.slug, n.kind, n.display_name, n.owner_id,
                n.vcpu_limit, n.mem_limit_mb, n.vm_limit, n.created_at
         FROM namespaces n
         JOIN namespace_members m ON n.id = m.namespace_id
         WHERE m.account_id = $1
         ORDER BY n.kind DESC, n.created_at ASC",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(row_to_namespace).collect())
}

pub async fn update_namespace_display_name(
    pool: &PgPool,
    namespace_id: &str,
    display_name: &str,
) -> Result<()> {
    sqlx::query("UPDATE namespaces SET display_name = $1 WHERE id = $2")
        .bind(display_name)
        .bind(namespace_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_namespace_members(pool: &PgPool, namespace_id: &str) -> Result<Vec<MemberRow>> {
    let rows = sqlx::query(
        "SELECT m.namespace_id, m.account_id, a.username, m.role, m.joined_at
         FROM namespace_members m
         JOIN accounts a ON a.id = m.account_id
         WHERE m.namespace_id = $1
         ORDER BY m.joined_at ASC",
    )
    .bind(namespace_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| MemberRow {
            namespace_id: r.get("namespace_id"),
            account_id: r.get("account_id"),
            username: r.get("username"),
            role: r.get("role"),
            joined_at: r.get("joined_at"),
        })
        .collect())
}

pub async fn remove_namespace_member(
    pool: &PgPool,
    namespace_id: &str,
    account_id: &str,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM namespace_members WHERE namespace_id = $1 AND account_id = $2",
    )
    .bind(namespace_id)
    .bind(account_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn count_namespace_owners(pool: &PgPool, namespace_id: &str) -> Result<i64> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM namespace_members WHERE namespace_id = $1 AND role = 'owner'",
    )
    .bind(namespace_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn get_namespace_member(
    pool: &PgPool,
    namespace_id: &str,
    account_id: &str,
) -> Result<Option<MemberRow>> {
    let row = sqlx::query(
        "SELECT m.namespace_id, m.account_id, a.username, m.role, m.joined_at
         FROM namespace_members m
         JOIN accounts a ON a.id = m.account_id
         WHERE m.namespace_id = $1 AND m.account_id = $2",
    )
    .bind(namespace_id)
    .bind(account_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| MemberRow {
        namespace_id: r.get("namespace_id"),
        account_id: r.get("account_id"),
        username: r.get("username"),
        role: r.get("role"),
        joined_at: r.get("joined_at"),
    }))
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

/// Check namespace quota and atomically set the VM to status='starting'.
/// Runs in a SERIALIZABLE transaction to prevent concurrent starts from racing.
/// Returns Err(QuotaError::Serialization) on conflict — caller should retry once.
pub async fn check_quota_and_reserve(
    pool: &PgPool,
    namespace_id: &str,
    vm_id: &str,
    vcpus: i64,
    mem_mb: i32,
) -> std::result::Result<(), QuotaError> {
    let mut tx = pool.begin().await.map_err(DbError::from)?;

    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *tx)
        .await
        .map_err(DbError::from)?;

    let ns_row =
        sqlx::query("SELECT vcpu_limit, mem_limit_mb, vm_limit FROM namespaces WHERE id = $1")
            .bind(namespace_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(DbError::from)?
            .ok_or_else(|| QuotaError::Exceeded("namespace not found".into()))?;

    let vcpu_limit: i64 = ns_row.get("vcpu_limit");
    let mem_limit: i32 = ns_row.get("mem_limit_mb");
    let vm_limit: i32 = ns_row.get("vm_limit");

    let usage_row = sqlx::query(
        "SELECT COALESCE(SUM(vcpus),0)::bigint AS used_vcpus,
                COALESCE(SUM(memory_mb),0)::int AS used_mem,
                COUNT(*)::int AS used_vms
         FROM vms
         WHERE namespace_id = $1 AND status IN ('running','starting')",
    )
    .bind(namespace_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(DbError::from)?;

    let used_vcpus: i64 = usage_row.get("used_vcpus");
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
        "SELECT id, account_id, name, status, subdomain, vcpus, memory_mb, disk_mb, bandwidth_mbps,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, tap_device, pid,
         socket_path, host_id, base_image, cloned_from, disk_usage_mb, created_at, last_started_at,
         placement_strategy, required_labels, region, namespace_id FROM vms WHERE account_id = $1 AND name = $2",
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

pub async fn update_vm_resources(
    pool: &PgPool,
    vm_id: &str,
    vcpus: i64,
    memory_mb: i32,
    bandwidth_mbps: i32,
) -> Result<()> {
    sqlx::query("UPDATE vms SET vcpus = $1, memory_mb = $2, bandwidth_mbps = $3 WHERE id = $4")
        .bind(vcpus)
        .bind(memory_mb)
        .bind(bandwidth_mbps)
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

// ── Images ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ImageRow {
    pub id: String,
    pub name: String,
    pub tag: String,
    pub source: String,
    pub status: String,
    pub size_bytes: i64,
    pub error: Option<String>,
    pub build_log: String,
    pub created_at: i64,
}

fn row_to_image(r: &sqlx::postgres::PgRow) -> ImageRow {
    ImageRow {
        id: r.get("id"),
        name: r.get("name"),
        tag: r.get("tag"),
        source: r.get("source"),
        status: r.get("status"),
        size_bytes: r.get("size_bytes"),
        error: r.get("error"),
        build_log: r.get("build_log"),
        created_at: r.get("created_at"),
    }
}

pub async fn create_image(
    pool: &PgPool,
    id: &str,
    name: &str,
    tag: &str,
    source: &str,
) -> Result<ImageRow> {
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    sqlx::query(
        "INSERT INTO images (id, name, tag, source, status, size_bytes, created_at)
         VALUES ($1, $2, $3, $4, 'building', 0, $5)",
    )
    .bind(id)
    .bind(name)
    .bind(tag)
    .bind(source)
    .bind(created_at)
    .execute(pool)
    .await?;
    Ok(get_image(pool, id).await?.expect("image just inserted"))
}

pub async fn get_image(pool: &PgPool, id: &str) -> Result<Option<ImageRow>> {
    let row = sqlx::query(
        "SELECT id, name, tag, source, status, size_bytes, error, build_log, created_at
         FROM images WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.as_ref().map(row_to_image))
}

pub async fn get_image_by_name_tag(
    pool: &PgPool,
    name: &str,
    tag: &str,
) -> Result<Option<ImageRow>> {
    let row = sqlx::query(
        "SELECT id, name, tag, source, status, size_bytes, error, build_log, created_at
         FROM images WHERE name = $1 AND tag = $2",
    )
    .bind(name)
    .bind(tag)
    .fetch_optional(pool)
    .await?;
    Ok(row.as_ref().map(row_to_image))
}

pub async fn list_images(pool: &PgPool) -> Result<Vec<ImageRow>> {
    let rows = sqlx::query(
        "SELECT id, name, tag, source, status, size_bytes, error, build_log, created_at
         FROM images ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(row_to_image).collect())
}

pub async fn update_image_ready(pool: &PgPool, id: &str, size_bytes: i64) -> Result<()> {
    sqlx::query("UPDATE images SET status = 'ready', size_bytes = $1, error = NULL WHERE id = $2")
        .bind(size_bytes)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_image_error(pool: &PgPool, id: &str, error: &str) -> Result<()> {
    sqlx::query("UPDATE images SET status = 'error', error = $1 WHERE id = $2")
        .bind(error)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn append_image_log(pool: &PgPool, id: &str, line: &str) -> Result<()> {
    sqlx::query("UPDATE images SET build_log = build_log || $1 WHERE id = $2")
        .bind(line)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_image(pool: &PgPool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM images WHERE id = $1")
        .bind(id)
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

pub async fn list_api_tokens(pool: &PgPool, account_id: &str) -> Result<Vec<ApiTokenRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, token_hash, name, created_at, last_used_at
         FROM api_tokens WHERE account_id = $1 ORDER BY created_at DESC",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ApiTokenRow {
            id: r.get("id"),
            account_id: r.get("account_id"),
            token_hash: r.get("token_hash"),
            name: r.get("name"),
            created_at: r.get("created_at"),
            last_used_at: r.get("last_used_at"),
        })
        .collect())
}

pub async fn delete_api_token(pool: &PgPool, id: &str, account_id: &str) -> Result<bool> {
    let result = sqlx::query(
        "DELETE FROM api_tokens WHERE id = $1 AND account_id = $2",
    )
    .bind(id)
    .bind(account_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

// ── SSH keys ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SshKeyRow {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub public_key: String,
    pub fingerprint: String,
    pub created_at: i64,
}

pub async fn add_ssh_key(
    pool: &PgPool,
    account_id: &str,
    name: &str,
    public_key: &str,
    fingerprint: &str,
) -> Result<SshKeyRow> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = unix_now();
    sqlx::query(
        "INSERT INTO ssh_keys (id, account_id, name, public_key, fingerprint, created_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&id)
    .bind(account_id)
    .bind(name)
    .bind(public_key)
    .bind(fingerprint)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(SshKeyRow {
        id,
        account_id: account_id.to_string(),
        name: name.to_string(),
        public_key: public_key.to_string(),
        fingerprint: fingerprint.to_string(),
        created_at: now,
    })
}

pub async fn list_ssh_keys(pool: &PgPool, account_id: &str) -> Result<Vec<SshKeyRow>> {
    let rows = sqlx::query(
        "SELECT id, account_id, name, public_key, fingerprint, created_at
         FROM ssh_keys WHERE account_id = $1 ORDER BY created_at ASC",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| SshKeyRow {
            id: r.get("id"),
            account_id: r.get("account_id"),
            name: r.get("name"),
            public_key: r.get("public_key"),
            fingerprint: r.get("fingerprint"),
            created_at: r.get("created_at"),
        })
        .collect())
}

pub async fn delete_ssh_key(pool: &PgPool, id: &str, account_id: &str) -> Result<bool> {
    let res = sqlx::query("DELETE FROM ssh_keys WHERE id = $1 AND account_id = $2")
        .bind(id)
        .bind(account_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn get_account_id_by_key_fingerprint(
    pool: &PgPool,
    fingerprint: &str,
) -> Result<Option<String>> {
    let row = sqlx::query("SELECT account_id FROM ssh_keys WHERE fingerprint = $1")
        .bind(fingerprint)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get("account_id")))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
