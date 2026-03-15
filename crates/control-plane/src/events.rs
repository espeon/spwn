use std::sync::Arc;
use std::time::Duration;

use agent_proto::agent::{WatchRequest, host_agent_client::HostAgentClient};
use serde::Serialize;
use tokio::sync::{Mutex, broadcast};
use tonic::transport::Channel;
use tracing::{info, warn};

use crate::caddy_router::CaddyRouter;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    VmStatus {
        vm_id: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        last_started_at: Option<i64>,
    },
    SnapshotComplete {
        vm_id: String,
        snap_id: String,
    },
}

impl AppEvent {
    /// SSE event name used on the wire.
    pub fn event_name(&self) -> &'static str {
        match self {
            AppEvent::VmStatus { .. } => "vm_status",
            AppEvent::SnapshotComplete { .. } => "snapshot_complete",
        }
    }
}

pub type EventBroadcast = broadcast::Sender<AppEvent>;

#[derive(Clone)]
pub struct EventWatcher {
    pool: db::PgPool,
    caddy: CaddyRouter,
    watched: Arc<Mutex<std::collections::HashSet<String>>>,
    pub tx: EventBroadcast,
}

impl EventWatcher {
    pub fn new(pool: db::PgPool, caddy: CaddyRouter) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            pool,
            caddy,
            watched: Arc::new(Mutex::new(std::collections::HashSet::new())),
            tx,
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
        let tx = self.tx.clone();
        let watcher = self.clone();
        tokio::spawn(async move {
            watch_loop(host_id, address, pool, caddy, tx, watcher).await;
        });
    }
}

async fn watch_loop(
    host_id: String,
    address: String,
    pool: db::PgPool,
    caddy: CaddyRouter,
    tx: EventBroadcast,
    watcher: EventWatcher,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        info!("connecting to host {host_id} for event stream ({address})");
        match connect_and_stream(&host_id, &address, &pool, &caddy, &tx).await {
            Ok(()) => {
                warn!("event stream for host {host_id} closed, reconnecting...");
            }
            Err(e) => {
                warn!("event stream error for host {host_id}: {e}, retrying in {backoff:?}");
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));

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
    caddy: &CaddyRouter,
    tx: &EventBroadcast,
) -> anyhow::Result<()> {
    let channel = Channel::from_shared(address.to_string())?.connect().await?;
    let mut client = HostAgentClient::new(channel);
    let mut stream = client.watch_events(WatchRequest {}).await?.into_inner();

    while let Some(event) = stream.message().await? {
        let vm_id = &event.vm_id;
        db::log_event(pool, vm_id, &event.event, Some(&event.detail))
            .await
            .ok();

        let app_event: Option<AppEvent> = match event.event.as_str() {
            "started" => {
                if let Ok(Some(vm)) = db::get_vm(pool, vm_id).await {
                    db::set_vm_status(pool, vm_id, "running").await.ok();
                    caddy
                        .broadcast_set_vm_route(
                            &vm.subdomain,
                            &vm.ip_address,
                            vm.exposed_port as u16,
                        )
                        .await;
                    Some(AppEvent::VmStatus {
                        vm_id: vm_id.clone(),
                        status: "running".into(),
                        last_started_at: vm.last_started_at,
                    })
                } else {
                    Some(AppEvent::VmStatus {
                        vm_id: vm_id.clone(),
                        status: "running".into(),
                        last_started_at: None,
                    })
                }
            }
            "stopped" => {
                if let Ok(Some(vm)) = db::get_vm(pool, vm_id).await {
                    db::set_vm_status(pool, vm_id, "stopped").await.ok();
                    caddy.broadcast_set_stopped_route(&vm.subdomain).await;
                }
                Some(AppEvent::VmStatus {
                    vm_id: vm_id.clone(),
                    status: "stopped".into(),
                    last_started_at: None,
                })
            }
            "crashed" => {
                if let Ok(Some(vm)) = db::get_vm(pool, vm_id).await {
                    db::set_vm_status(pool, vm_id, "error").await.ok();
                    caddy.broadcast_set_stopped_route(&vm.subdomain).await;
                }
                Some(AppEvent::VmStatus {
                    vm_id: vm_id.clone(),
                    status: "error".into(),
                    last_started_at: None,
                })
            }
            "snapshot_taken" => {
                // Snapshot is complete; VM should be running again.
                db::set_vm_status(pool, vm_id, "running").await.ok();
                let snap_id = event.detail.clone();
                // Emit both a status update and a snapshot notification.
                let _ = tx.send(AppEvent::VmStatus {
                    vm_id: vm_id.clone(),
                    status: "running".into(),
                    last_started_at: None,
                });
                Some(AppEvent::SnapshotComplete {
                    vm_id: vm_id.clone(),
                    snap_id,
                })
            }
            _ => None,
        };

        if let Some(ev) = app_event {
            let _ = tx.send(ev);
        }

        info!("[{}] event: {} {}", host_id, event.event, vm_id);
    }

    Ok(())
}
