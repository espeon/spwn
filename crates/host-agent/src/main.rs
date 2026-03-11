use std::{path::PathBuf, sync::Arc, time::Duration};

use agent_proto::agent::{
    HeartbeatRequest, RegisterRequest, control_plane_client::ControlPlaneClient,
};
use anyhow::Context;
use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use fctools::vmm::installation::VmmInstallation;
use networking::NetworkManager;
use tonic::transport::Channel;
use tracing::info;

mod agent;
mod health;
mod manager;
mod overlay;
mod reconcile;

use manager::VmManager;

fn firecracker_path() -> PathBuf {
    std::env::var("FIRECRACKER_BIN")
        .unwrap_or_else(|_| "/usr/local/bin/firecracker".into())
        .into()
}

fn jailer_path() -> PathBuf {
    std::env::var("JAILER_BIN")
        .unwrap_or_else(|_| "/usr/local/bin/jailer".into())
        .into()
}

fn jailer_uid() -> anyhow::Result<u32> {
    if let Ok(val) = std::env::var("JAILER_UID") {
        return val.parse::<u32>().context("parse JAILER_UID");
    }
    resolve_user_id("spwn-vm")
}

fn jailer_gid() -> anyhow::Result<u32> {
    if let Ok(val) = std::env::var("JAILER_GID") {
        return val.parse::<u32>().context("parse JAILER_GID");
    }
    resolve_group_id("spwn-vm")
}

fn resolve_user_id(name: &str) -> anyhow::Result<u32> {
    let contents = std::fs::read_to_string("/etc/passwd").context("read /etc/passwd")?;
    parse_id_from_passwd(&contents, name).ok_or_else(|| {
        anyhow::anyhow!("user '{name}' not found in /etc/passwd — create it or set JAILER_UID")
    })
}

fn resolve_group_id(name: &str) -> anyhow::Result<u32> {
    let contents = std::fs::read_to_string("/etc/group").context("read /etc/group")?;
    parse_id_from_passwd(&contents, name).ok_or_else(|| {
        anyhow::anyhow!("group '{name}' not found in /etc/group — create it or set JAILER_GID")
    })
}

// Parse a uid or gid from a colon-delimited passwd/group file buffer.
// Both formats share the same structure for the fields we care about:
//   name:password:id:...
fn parse_id_from_passwd(contents: &str, name: &str) -> Option<u32> {
    for line in contents.lines() {
        let mut fields = line.splitn(4, ':');
        let Some(entry_name) = fields.next() else {
            continue;
        };
        let Some(_password) = fields.next() else {
            continue;
        };
        let Some(id_str) = fields.next() else {
            continue;
        };
        if entry_name == name {
            return id_str.parse::<u32>().ok();
        }
    }
    None
}

fn snapshot_editor_path() -> PathBuf {
    std::env::var("SNAPSHOT_EDITOR_BIN")
        .unwrap_or_else(|_| "/usr/local/bin/snapshot-editor".into())
        .into()
}

fn load_or_create_host_id() -> anyhow::Result<String> {
    let path = std::path::Path::new("/var/lib/spwn/host-id");
    if let Ok(id) = std::fs::read_to_string(path) {
        return Ok(id.trim().to_string());
    }
    let id = uuid::Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, &id).ok();
    Ok(id)
}

#[derive(Clone)]
struct SnapshotServerState {
    overlay_dir: PathBuf,
    agent_secret: String,
}

async fn serve_overlay(
    State(state): State<SnapshotServerState>,
    Path(vm_id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let authed = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t == state.agent_secret)
        .unwrap_or(false);

    if !authed {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // Sanitize: vm_id must be a UUID (no path traversal).
    if vm_id.contains('/') || vm_id.contains("..") {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let path = state.overlay_dir.join(format!("{vm_id}.ext4"));
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [("content-type", "application/octet-stream")],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".into()),
        )
        .init();

    let host_id = match std::env::var("HOST_ID") {
        Ok(id) => id,
        Err(_) => load_or_create_host_id()?,
    };
    let host_name = std::env::var("HOST_NAME").unwrap_or_else(|_| hostname());
    let agent_listen_addr =
        std::env::var("AGENT_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:4000".into());
    let agent_public_addr = std::env::var("AGENT_PUBLIC_ADDR")
        .expect("AGENT_PUBLIC_ADDR must be set (e.g. http://localhost:4000)");
    let control_plane_url = std::env::var("CONTROL_PLANE_URL")
        .expect("CONTROL_PLANE_URL must be set (e.g. http://localhost:5000)");
    let snapshot_listen_addr =
        std::env::var("SNAPSHOT_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:4001".into());
    let snapshot_public_addr =
        std::env::var("SNAPSHOT_PUBLIC_ADDR").unwrap_or_else(|_| "http://localhost:4001".into());
    let agent_secret = std::env::var("AGENT_SECRET").unwrap_or_default();

    let kernel_path: PathBuf = std::env::var("KERNEL_PATH")
        .expect("KERNEL_PATH must be set")
        .into();
    let images_dir: PathBuf = std::env::var("IMAGES_DIR")
        .unwrap_or_else(|_| "/var/lib/spwn/images".into())
        .into();
    let overlay_dir: PathBuf = std::env::var("OVERLAY_DIR")
        .unwrap_or_else(|_| "/var/lib/spwn/overlays".into())
        .into();
    let snapshot_dir: PathBuf = std::env::var("SNAPSHOT_DIR")
        .unwrap_or_else(|_| "/var/lib/spwn/snapshots".into())
        .into();
    let chroot_base_dir: PathBuf = std::env::var("JAILER_CHROOT_BASE")
        .unwrap_or_else(|_| "/srv/jailer".into())
        .into();
    let jailer_uid = jailer_uid().context("resolve jailer UID")?;
    let jailer_gid = jailer_gid().context("resolve jailer GID")?;
    info!(
        "jailer uid={jailer_uid} gid={jailer_gid} chroot_base={}",
        chroot_base_dir.display()
    );
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:spwn@localhost/spwn".into());

    let external_iface = match std::env::var("EXTERNAL_IFACE") {
        Ok(iface) => iface,
        Err(_) => {
            let iface = networking::iptables::default_route_iface()?;
            info!("auto-detected external interface: {iface}");
            iface
        }
    };

    networking::iptables::enable_ip_forwarding()?;
    networking::iptables::setup(&external_iface)?;
    setup_cgroup_controllers()?;

    for dir in [&overlay_dir, &images_dir, &snapshot_dir] {
        std::fs::create_dir_all(dir).with_context(|| format!("create dir: {}", dir.display()))?;
    }

    info!("connecting to database");
    let pool = db::connect(&database_url).await?;
    db::migrate(&pool).await?;
    info!("migrations complete");

    std::fs::create_dir_all(&chroot_base_dir)
        .with_context(|| format!("create chroot base dir: {}", chroot_base_dir.display()))?;

    let manager = Arc::new(VmManager::new(
        pool,
        NetworkManager::new(),
        VmmInstallation::new(firecracker_path(), jailer_path(), snapshot_editor_path()),
        kernel_path.clone(),
        images_dir.clone(),
        overlay_dir.clone(),
        snapshot_dir.clone(),
        host_id.clone(),
        jailer_uid,
        jailer_gid,
        chroot_base_dir,
    ));

    reconcile::reconcile_once(&manager).await?;

    tokio::spawn(reconcile::run_reconciliation(manager.clone()));
    tokio::spawn(health::run_health_checks(manager.clone()));

    // Snapshot HTTP server — serves overlay files to peer agents for migration.
    let snap_state = SnapshotServerState {
        overlay_dir: overlay_dir.clone(),
        agent_secret: agent_secret.clone(),
    };
    let snap_app = Router::new()
        .route("/overlay/{vm_id}", get(serve_overlay))
        .with_state(snap_state);
    let snap_listener = tokio::net::TcpListener::bind(&snapshot_listen_addr).await?;
    info!("snapshot HTTP server listening on {snapshot_listen_addr}");
    tokio::spawn(async move {
        if let Err(e) = axum::serve(snap_listener, snap_app).await {
            tracing::error!("snapshot HTTP server error: {e}");
        }
    });

    // Register with control plane.
    let cp_channel = Channel::from_shared(control_plane_url.clone())
        .context("parse control plane URL")?
        .connect_lazy();
    let mut cp_client = ControlPlaneClient::new(cp_channel);

    cp_client
        .register(RegisterRequest {
            host_id: host_id.clone(),
            name: host_name.clone(),
            address: agent_public_addr.clone(),
            vcpu_total: num_cpus() as u64 * 1000,
            mem_total_mb: total_mem_mb(),
            images_dir: images_dir.to_string_lossy().into(),
            overlay_dir: overlay_dir.to_string_lossy().into(),
            snapshot_dir: snapshot_dir.to_string_lossy().into(),
            kernel_path: kernel_path.to_string_lossy().into(),
            snapshot_addr: snapshot_public_addr.clone(),
        })
        .await
        .context("register with control plane")?;

    info!("registered with control plane at {control_plane_url}");

    // Heartbeat loop — reports real resource usage.
    let hb_manager = manager.clone();
    let hb_host_id = host_id.clone();
    let hb_cp_url = control_plane_url.clone();
    tokio::spawn(async move {
        let channel = Channel::from_shared(hb_cp_url)
            .expect("valid URL")
            .connect_lazy();
        let mut client = ControlPlaneClient::new(channel);
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let vms = db::get_vms_by_host(&hb_manager.pool, &hb_host_id)
                .await
                .unwrap_or_default();
            let running: Vec<_> = vms.into_iter().filter(|v| v.status == "running").collect();
            let vcpu_used: u64 = running.iter().map(|v| v.vcpus as u64).sum();
            let mem_used_mb: u32 = running.iter().map(|v| v.memory_mb as u32).sum();
            let running_ids = running.into_iter().map(|v| v.id).collect();
            let _ = client
                .heartbeat(HeartbeatRequest {
                    host_id: hb_host_id.clone(),
                    running_vm_ids: running_ids,
                    vcpu_used,
                    mem_used_mb,
                })
                .await;
        }
    });

    // gRPC server.
    use agent_proto::agent::host_agent_server::HostAgentServer;
    let svc = agent::HostAgentService {
        manager: manager.clone(),
        agent_secret,
    };
    let listen: std::net::SocketAddr = agent_listen_addr
        .parse()
        .context("parse AGENT_LISTEN_ADDR")?;

    info!("host-agent gRPC listening on {agent_listen_addr}");

    tokio::select! {
        result = tonic::transport::Server::builder()
            .add_service(HostAgentServer::new(svc))
            .serve(listen) => { result?; }
        _ = tokio::signal::ctrl_c() => {
            info!("received ctrl-c, shutting down");
        }
    }

    manager.shutdown().await;
    Ok(())
}

// Enable cpu and memory controllers in the jailer cgroup hierarchy.
// cgroupv2 requires the controllers to be listed in cgroup.subtree_control
// of every ancestor before child cgroups can use them.
fn setup_cgroup_controllers() -> anyhow::Result<()> {
    let cgroup_dir = std::path::Path::new("/sys/fs/cgroup/firecracker");
    std::fs::create_dir_all(cgroup_dir)
        .with_context(|| format!("create cgroup dir {}", cgroup_dir.display()))?;
    let subtree_control = cgroup_dir.join("cgroup.subtree_control");
    std::fs::write(&subtree_control, "+cpu +memory").with_context(|| {
        format!(
            "enable cpu+memory controllers in {}",
            subtree_control.display()
        )
    })?;
    Ok(())
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

fn num_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::parse_id_from_passwd;

    const PASSWD: &str = "\
root:x:0:0:root:/root:/bin/bash
daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin
spwn-vm:x:954:954::/home/spwn-vm:/sbin/nologin
nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin
";

    const GROUP: &str = "\
root:x:0:
daemon:x:1:
spwn-vm:x:954:
nogroup:x:65534:
";

    #[test]
    fn parse_id_finds_existing_user() {
        assert_eq!(parse_id_from_passwd(PASSWD, "spwn-vm"), Some(954));
    }

    #[test]
    fn parse_id_finds_root() {
        assert_eq!(parse_id_from_passwd(PASSWD, "root"), Some(0));
    }

    #[test]
    fn parse_id_finds_high_uid() {
        assert_eq!(parse_id_from_passwd(PASSWD, "nobody"), Some(65534));
    }

    #[test]
    fn parse_id_returns_none_for_missing_user() {
        assert_eq!(parse_id_from_passwd(PASSWD, "nonexistent"), None);
    }

    #[test]
    fn parse_id_returns_none_for_empty_input() {
        assert_eq!(parse_id_from_passwd("", "spwn-vm"), None);
    }

    #[test]
    fn parse_id_works_for_group_file() {
        assert_eq!(parse_id_from_passwd(GROUP, "spwn-vm"), Some(954));
        assert_eq!(parse_id_from_passwd(GROUP, "nogroup"), Some(65534));
    }

    #[test]
    fn parse_id_does_not_match_partial_name() {
        assert_eq!(parse_id_from_passwd(PASSWD, "spwn"), None);
        assert_eq!(parse_id_from_passwd(PASSWD, "spwn-vm-extra"), None);
    }

    #[test]
    fn parse_id_skips_lines_missing_id_field() {
        // a line with only one colon-separated field means the id field is
        // missing; the iterator returns None for fields.next() on the id
        // position and the line is skipped. the next valid line is found.
        let malformed = "no-password-or-id\nspwn-vm:x:954:954::/home/spwn-vm:/sbin/nologin\n";
        assert_eq!(parse_id_from_passwd(malformed, "spwn-vm"), Some(954));
    }
}

fn total_mem_mb() -> u32 {
    let Ok(info) = std::fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    for line in info.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            if let Some(kb) = rest.trim().split_whitespace().next() {
                if let Ok(kb) = kb.parse::<u32>() {
                    return kb / 1024;
                }
            }
        }
    }
    0
}
