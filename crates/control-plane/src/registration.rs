use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::Mutex;
use tonic::{Request, Response, Status};
use tracing::warn;

use agent_proto::agent::{
    control_plane_server::ControlPlane,
    HeartbeatRequest, HeartbeatResponse,
    RegisterRequest, RegisterResponse,
};

use crate::{caddy_router::CaddyRouter, events::EventWatcher};

pub struct ControlPlaneService {
    pub pool: db::PgPool,
    pub event_watcher: EventWatcher,
    pub caddy: CaddyRouter,
    pub base_domain: String,
    /// Last known running VM set per host — Caddy is only synced when this changes.
    pub running_cache: Arc<Mutex<HashMap<String, HashSet<String>>>>,
}

#[tonic::async_trait]
impl ControlPlane for ControlPlaneService {
    async fn register(
        &self,
        req: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let r = req.into_inner();
        let host = db::NewHost {
            id: r.host_id.clone(),
            name: r.name,
            address: r.address.clone(),
            vcpu_total: r.vcpu_total as i64,
            mem_total_mb: r.mem_total_mb as i32,
            images_dir: r.images_dir,
            overlay_dir: r.overlay_dir,
            snapshot_dir: r.snapshot_dir,
            kernel_path: r.kernel_path,
            snapshot_addr: r.snapshot_addr,
        };
        match db::upsert_host(&self.pool, &host).await {
            Ok(_) => {
                tracing::info!("host {} registered ({})", r.host_id, r.address);
                self.event_watcher.watch_host(r.host_id, r.address).await;
                Ok(Response::new(RegisterResponse { ok: true }))
            }
            Err(e) => {
                tracing::error!("failed to register host {}: {e}", r.host_id);
                Ok(Response::new(RegisterResponse { ok: false }))
            }
        }
    }

    async fn heartbeat(
        &self,
        req: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let r = req.into_inner();
        db::update_host_heartbeat(
            &self.pool,
            &r.host_id,
            r.vcpu_used as i64,
            r.mem_used_mb as i32,
        )
        .await
        .ok();

        let new_set: HashSet<String> = r.running_vm_ids.iter().cloned().collect();
        let changed = {
            let mut cache = self.running_cache.lock().await;
            let prev = cache.entry(r.host_id.clone()).or_default();
            if *prev != new_set {
                *prev = new_set.clone();
                true
            } else {
                false
            }
        };

        if changed {
            self.reconcile_from_heartbeat(&r.host_id, &new_set).await;
        }

        Ok(Response::new(HeartbeatResponse {}))
    }
}

impl ControlPlaneService {
    async fn reconcile_from_heartbeat(&self, host_id: &str, running: &HashSet<String>) {
        let db_vms = match db::get_vms_by_host(&self.pool, host_id).await {
            Ok(vms) => vms,
            Err(e) => {
                warn!("heartbeat reconcile: failed to fetch VMs for host {host_id}: {e}");
                return;
            }
        };

        let host = db::get_host(&self.pool, host_id).await.ok().flatten();

        for vm in &db_vms {
            let actually_running = running.contains(&vm.id);
            let fqdn = format!("{}.{}", vm.subdomain, self.base_domain);
            let caddy_client = match &host {
                Some(h) => self.caddy.for_host(h),
                None => self.caddy.for_region(None),
            };

            match vm.status.as_str() {
                "running" if !actually_running => {
                    warn!(
                        "vm {} is 'running' in DB but absent from heartbeat — marking error",
                        vm.id
                    );
                    db::set_vm_status(&self.pool, &vm.id, "error").await.ok();
                    db::log_event(&self.pool, &vm.id, "heartbeat_process_gone", None).await.ok();
                    caddy_client.set_stopped_route(&fqdn).await.ok();
                }
                "running" if actually_running => {
                    // VM is correctly running — sync Caddy unconditionally so a
                    // missed event never leaves a stale stopped route in place.
                    if let Err(e) = caddy_client
                        .set_vm_route(&fqdn, &vm.ip_address, vm.exposed_port as u16)
                        .await
                    {
                        warn!("heartbeat caddy sync failed for {}: {e}", vm.id);
                    }
                }
                "stopped" | "error" if actually_running => {
                    warn!(
                        "vm {} is '{}' in DB but running on host — recovering via heartbeat",
                        vm.id, vm.status
                    );
                    db::set_vm_status(&self.pool, &vm.id, "running").await.ok();
                    db::log_event(&self.pool, &vm.id, "heartbeat_recovery", None).await.ok();
                    if let Err(e) = caddy_client
                        .set_vm_route(&fqdn, &vm.ip_address, vm.exposed_port as u16)
                        .await
                    {
                        warn!("heartbeat recovery: failed to set caddy route for {}: {e}", vm.id);
                    }
                }
                _ => {}
            }
        }
    }
}
