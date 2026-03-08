use std::sync::Arc;
use std::time::Duration;

use agent_proto::agent::{
    host_agent_client::HostAgentClient,
    WatchRequest,
};
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tracing::{error, info, warn};

use router_sync::CaddyClient;

#[derive(Clone)]
pub struct EventWatcher {
    pool: db::PgPool,
    caddy: CaddyClient,
    watched: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl EventWatcher {
    pub fn new(pool: db::PgPool, caddy: CaddyClient) -> Self {
        Self {
            pool,
            caddy,
            watched: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }

    pub async fn watch_host(&self, host_id: String, address: String) {
        let mut watched = self.watched.lock().await;
        if watched.contains(&host_id) {
            return;
        }
        watched.insert(host_id.clone());
        drop(watched);

        let pool = self.pool.clone();
        let caddy = self.caddy.clone();
        let watcher = self.clone();
        tokio::spawn(async move {
            watch_loop(host_id, address, pool, caddy, watcher).await;
        });
    }
}

async fn watch_loop(
    host_id: String,
    address: String,
    pool: db::PgPool,
    caddy: CaddyClient,
    watcher: EventWatcher,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        info!("connecting to host {host_id} for event stream ({address})");
        match connect_and_stream(&host_id, &address, &pool, &caddy).await {
            Ok(()) => {
                warn!("event stream for host {host_id} closed, reconnecting...");
            }
            Err(e) => {
                warn!("event stream error for host {host_id}: {e}, retrying in {backoff:?}");
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));

        // re-check host is still registered before retrying
        match db::get_host(&pool, &host_id).await {
            Ok(None) => {
                info!("host {host_id} no longer in DB, stopping watcher");
                watcher.watched.lock().await.remove(&host_id);
                return;
            }
            _ => {}
        }
    }
}

async fn connect_and_stream(
    host_id: &str,
    address: &str,
    pool: &db::PgPool,
    caddy: &CaddyClient,
) -> anyhow::Result<()> {
    let channel = Channel::from_shared(address.to_string())?
        .connect()
        .await?;
    let mut client = HostAgentClient::new(channel);
    let mut stream = client.watch_events(WatchRequest {}).await?.into_inner();

    while let Some(event) = stream.message().await? {
        let vm_id = &event.vm_id;
        db::log_event(pool, vm_id, &event.event, Some(&event.detail)).await.ok();

        match event.event.as_str() {
            "started" => {
                if let Ok(Some(vm)) = db::get_vm(pool, vm_id).await {
                    db::set_vm_status(pool, vm_id, "running").await.ok();
                    if let Err(e) = caddy.set_vm_route(&vm.subdomain, &vm.ip_address, vm.exposed_port as u16).await {
                        error!("failed to set caddy route for {vm_id}: {e}");
                    }
                }
            }
            "stopped" => {
                if let Ok(Some(vm)) = db::get_vm(pool, vm_id).await {
                    db::set_vm_status(pool, vm_id, "stopped").await.ok();
                    if let Err(e) = caddy.set_stopped_route(&vm.subdomain).await {
                        error!("failed to set stopped caddy route for {vm_id}: {e}");
                    }
                }
            }
            "crashed" => {
                if let Ok(Some(vm)) = db::get_vm(pool, vm_id).await {
                    db::set_vm_status(pool, vm_id, "error").await.ok();
                    if let Err(e) = caddy.set_stopped_route(&vm.subdomain).await {
                        error!("failed to update caddy route for crashed {vm_id}: {e}");
                    }
                }
            }
            _ => {}
        }

        info!("[{}] event: {} {}", host_id, event.event, vm_id);
    }

    Ok(())
}
