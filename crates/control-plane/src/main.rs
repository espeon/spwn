use std::{collections::HashMap, convert::Infallible, path::PathBuf, str::FromStr, sync::Arc};

use crate::caddy_router::CaddyRouter;
use agent_proto::agent::control_plane_server::ControlPlaneServer;
use anyhow::Context;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{Extension, routing::get};
use router_sync::CaddyClient;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};
use tower_http::services::{ServeDir, ServeFile};
use tracing::info;

mod admin;
mod caddy_router;
mod events;
mod migration;
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

    let listen_addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".into());
    let grpc_listen_addr =
        std::env::var("GRPC_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:5000".into());
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:spwn@localhost/spwn".into());
    let caddy_url = std::env::var("CADDY_URL").unwrap_or_else(|_| "http://localhost:2019".into());
    let static_files_path = PathBuf::from_str(
        &std::env::var("STATIC_FILES_PATH").unwrap_or_else(|_| "/var/lib/spwn/static".into()),
    )
    .expect("STATIC_FILES_PATH must be a valid path");
    let invite_code = std::env::var("INVITE_CODE").context("INVITE_CODE env var is required")?;
    let frontend_path = std::env::var("FRONTEND_PATH").unwrap_or_else(|_| "frontend/dist".into());
    let session_ttl_secs: i64 = std::env::var("SESSION_TTL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(604800);
    let public_url = std::env::var("PUBLIC_URL").unwrap_or_else(|_| "https://spwn.run".into());
    let gateway_secret = std::env::var("GATEWAY_SECRET").ok();
    let ssh_gateway_addr =
        std::env::var("SSH_GATEWAY_ADDR").unwrap_or_else(|_| "localhost:2222".into());
    info!("connecting to database");
    let pool = db::connect(&database_url).await?;
    db::migrate(&pool).await?;
    info!("migrations complete");

    let caddy_default = CaddyClient::new(&caddy_url, static_files_path.clone());
    caddy_default.write_static_files()?;

    let caddy_region_clients: HashMap<String, CaddyClient> = std::env::var("CADDY_REGION_URLS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let (region, url) = entry.split_once('=')?;
            Some((
                region.trim().to_string(),
                CaddyClient::new(url.trim(), static_files_path.clone()),
            ))
        })
        .collect();

    if !caddy_region_clients.is_empty() {
        info!(
            "caddy region overrides: {}",
            caddy_region_clients
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let caddy = CaddyRouter::new(caddy_default, caddy_region_clients);

    rebuild_caddy_routes(&pool, &caddy).await;

    let event_watcher = events::EventWatcher::new(pool.clone(), caddy.clone());
    let event_tx = event_watcher.tx.clone();

    let hosts = db::list_hosts(&pool).await.unwrap_or_default();
    for host in hosts {
        event_watcher.watch_host(host.id, host.address).await;
    }

    let ops = Arc::new(ops::ControlPlaneOps {
        pool: pool.clone(),
        caddy: caddy.clone(),
    });

    let grpc_svc = registration::ControlPlaneService {
        pool: pool.clone(),
        event_watcher,
    };

    let auth_state = auth::routes::AuthState {
        pool: pool.clone(),
        invite_code,
        session_ttl_secs,
        public_url,
        gateway_secret,
        ssh_gateway_addr,
    };

    let admin_state = admin::AdminState {
        pool: pool.clone(),
        caddy: caddy.clone(),
    };

    tokio::spawn(migration::run_drain_watcher(pool.clone(), caddy.clone()));

    let http_app = axum::Router::new()
        .merge(auth::auth_router(auth_state))
        .merge(api::router(ops as Arc<dyn api::VmOps>))
        .merge(admin::router(admin_state))
        .route("/api/events", get(vm_events_sse))
        .fallback_service(
            ServeDir::new(&frontend_path)
                .not_found_service(ServeFile::new(format!("{frontend_path}/index.html"))),
        )
        .layer(Extension(pool.clone()))
        .layer(Extension(event_tx));

    let http_listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    info!("control-plane HTTP listening on {listen_addr}");

    let grpc_listen: std::net::SocketAddr =
        grpc_listen_addr.parse().context("parse GRPC_LISTEN_ADDR")?;
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

async fn vm_events_sse(
    _account_id: auth::AccountId,
    Extension(tx): Extension<events::EventBroadcast>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(tx.subscribe()).filter_map(|result| match result {
        Ok(event) => serde_json::to_string(&event)
            .ok()
            .map(|data| Ok(Event::default().event("vm_status").data(data))),
        Err(_) => None,
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn rebuild_caddy_routes(pool: &db::PgPool, caddy: &CaddyRouter) {
    let vms = match db::get_all_vms(pool).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("failed to load vms for caddy rebuild: {e}");
            return;
        }
    };

    // Group VMs by their host's region so each regional caddy gets only its
    // own routes. VMs without a host (or whose host has no region label) fall
    // through to the default caddy.
    let hosts: HashMap<String, db::HostRow> = db::list_hosts(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|h| (h.id.clone(), h))
        .collect();

    // Collect (caddy_client, RouteEntry) pairs.
    let mut by_client: HashMap<String, (CaddyClient, Vec<router_sync::RouteEntry>)> =
        HashMap::new();

    for vm in vms {
        let host = vm.host_id.as_deref().and_then(|id| hosts.get(id));
        let client = match host {
            Some(h) => caddy.for_host(h),
            None => caddy.for_region(None),
        };
        let key = client.base_url().to_string();
        let target = if vm.status == "running" {
            router_sync::RouteTarget::Vm {
                ip: vm.ip_address.clone(),
                port: vm.exposed_port as u16,
            }
        } else {
            router_sync::RouteTarget::Stopped
        };
        by_client
            .entry(key)
            .or_insert_with(|| (client, Vec::new()))
            .1
            .push(router_sync::RouteEntry {
                subdomain: vm.subdomain,
                target,
            });
    }

    // Any caddy instance that has no VMs still needs a rebuild call to clear
    // stale dynamic config from a previous run.
    for (_, client) in caddy.all_regions() {
        let key = client.base_url().to_string();
        by_client
            .entry(key)
            .or_insert_with(|| (client.clone(), Vec::new()));
    }

    for (_, (client, routes)) in by_client {
        if let Err(e) = client.rebuild_all_routes(&routes).await {
            tracing::error!(
                "failed to rebuild caddy routes for {}: {e}",
                client.base_url()
            );
        }
    }
}
