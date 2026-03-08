use std::{path::PathBuf, str::FromStr, sync::Arc};

use anyhow::Context;
use agent_proto::agent::control_plane_server::ControlPlaneServer;
use router_sync::CaddyClient;
use tracing::info;

mod events;
mod ops;
mod registration;
mod scheduler;
mod subdomain;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".into()),
        )
        .init();

    let listen_addr = std::env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".into());
    let grpc_listen_addr = std::env::var("GRPC_LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:5000".into());
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:spwn@localhost/spwn".into());
    let caddy_url = std::env::var("CADDY_URL")
        .unwrap_or_else(|_| "http://localhost:2019".into());
    let static_files_path = PathBuf::from_str(
        &std::env::var("STATIC_FILES_PATH")
            .unwrap_or_else(|_| "/var/lib/spwn/static".into()),
    )
    .expect("STATIC_FILES_PATH must be a valid path");

    info!("connecting to database");
    let pool = db::connect(&database_url).await?;
    db::migrate(&pool).await?;
    info!("migrations complete");

    let caddy = CaddyClient::new(&caddy_url, static_files_path);
    caddy.write_static_files()?;

    // rebuild caddy routes from DB state
    rebuild_caddy_routes(&pool, &caddy).await;

    // event watcher for host → control plane streams
    let event_watcher = events::EventWatcher::new(pool.clone(), caddy.clone());

    // start watching events from all already-registered hosts
    let hosts = db::list_hosts(&pool).await.unwrap_or_default();
    for host in hosts {
        event_watcher.watch_host(host.id, host.address).await;
    }

    let ops = Arc::new(ops::ControlPlaneOps {
        pool: pool.clone(),
        caddy,
    });

    let grpc_svc = registration::ControlPlaneService {
        pool: pool.clone(),
        event_watcher,
    };

    let http_app = api::router(ops as Arc<dyn api::VmOps>);
    let http_listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    info!("control-plane HTTP listening on {listen_addr}");

    let grpc_listen: std::net::SocketAddr = grpc_listen_addr.parse()
        .context("parse GRPC_LISTEN_ADDR")?;
    info!("control-plane gRPC listening on {grpc_listen_addr}");

    tokio::select! {
        result = axum::serve(http_listener, http_app) => { result?; }
        result = tonic::transport::Server::builder()
            .add_service(ControlPlaneServer::new(grpc_svc))
            .serve(grpc_listen) => { result?; }
        _ = tokio::signal::ctrl_c() => {
            info!("received ctrl-c, shutting down");
        }
    }

    Ok(())
}

async fn rebuild_caddy_routes(pool: &db::PgPool, caddy: &CaddyClient) {
    let vms = match db::get_all_vms(pool).await {
        Ok(v) => v,
        Err(e) => { tracing::error!("failed to load vms for caddy rebuild: {e}"); return; }
    };

    let routes: Vec<router_sync::RouteEntry> = vms.into_iter().map(|vm| {
        let target = if vm.status == "running" {
            router_sync::RouteTarget::Vm { ip: vm.ip_address.clone(), port: vm.exposed_port as u16 }
        } else {
            router_sync::RouteTarget::Stopped
        };
        router_sync::RouteEntry { subdomain: vm.subdomain, target }
    }).collect();

    if let Err(e) = caddy.rebuild_all_routes(&routes).await {
        tracing::error!("failed to rebuild caddy routes: {e}");
    }
}
