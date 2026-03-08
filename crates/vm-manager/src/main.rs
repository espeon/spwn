use std::{path::PathBuf, str::FromStr, sync::Arc};

use anyhow::Context;
use fctools::vmm::installation::VmmInstallation;
use networking::NetworkManager;
use router_sync::CaddyClient;
use tracing::info;

mod health;
mod manager;
mod overlay;
mod reconcile;
mod subdomain;

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

fn snapshot_editor_path() -> PathBuf {
    std::env::var("SNAPSHOT_EDITOR_BIN")
        .unwrap_or_else(|_| "/usr/local/bin/snapshot-editor".into())
        .into()
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

    let kernel_path: PathBuf = std::env::var("KERNEL_PATH")
        .expect("KERNEL_PATH must be set")
        .into();
    let images_dir: PathBuf = std::env::var("IMAGES_DIR")
        .unwrap_or_else(|_| "/var/lib/spwn/images".into())
        .into();
    let overlay_dir: PathBuf = std::env::var("OVERLAY_DIR")
        .unwrap_or_else(|_| "/var/lib/spwn/overlays".into())
        .into();
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:spwn@localhost/spwn".into());
    let caddy_url = std::env::var("CADDY_URL")
        .unwrap_or_else(|_| "http://localhost:2019".into());
    let static_files_path = PathBuf::from_str(
        &std::env::var("STATIC_FILES_PATH")
            .unwrap_or_else(|_| "/var/lib/spwn/static".into()),
    )
    .expect("STATIC_FILES_PATH must be a valid path");
    let listen_addr = std::env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".into());
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

    std::fs::create_dir_all(&overlay_dir)
        .with_context(|| format!("create overlay dir: {}", overlay_dir.display()))?;
    info!("overlay dir: {}", overlay_dir.display());

    info!("connecting to database");
    let pool = db::connect(&database_url).await?;
    db::migrate(&pool).await?;
    info!("migrations complete");

    let caddy = CaddyClient::new(&caddy_url, static_files_path);
    caddy.write_static_files()?;

    std::fs::create_dir_all(&images_dir)
        .with_context(|| format!("create images dir: {}", images_dir.display()))?;
    info!("images dir: {}", images_dir.display());

    let manager = Arc::new(VmManager::new(
        pool,
        NetworkManager::new(),
        caddy,
        VmmInstallation::new(firecracker_path(), jailer_path(), snapshot_editor_path()),
        kernel_path,
        images_dir,
        overlay_dir,
    ));

    reconcile::reconcile_once(&manager).await?;

    tokio::spawn(reconcile::run_reconciliation(manager.clone()));
    tokio::spawn(health::run_health_checks(manager.clone()));

    let app = api::router(manager.clone() as Arc<dyn api::VmOps>);
    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    info!("listening on {listen_addr}");

    tokio::select! {
        result = axum::serve(listener, app) => { result?; }
        _ = tokio::signal::ctrl_c() => {
            info!("received ctrl-c, shutting down");
        }
    }

    manager.shutdown().await;
    Ok(())
}
