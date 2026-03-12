use anyhow::anyhow;
use async_trait::async_trait;
use tonic::transport::Channel;
use tracing::{error, warn};

use agent_proto::agent::{
    CloneVmRequest, CreateVmRequest, DeleteVmRequest, ResizeBandwidthRequest, ResizeCpuRequest,
    RestoreRequest, StartVmRequest, StopVmRequest, TakeSnapshotRequest,
    host_agent_client::HostAgentClient,
};
use router_sync::CaddyClient;

use crate::{caddy_router::CaddyRouter, scheduler, subdomain};

pub struct ControlPlaneOps {
    pub pool: db::PgPool,
    pub caddy: CaddyRouter,
}

impl ControlPlaneOps {
    async fn caddy_for_vm(&self, vm: &db::VmRow) -> CaddyClient {
        if let Some(host_id) = &vm.host_id {
            if let Ok(Some(h)) = db::get_host(&self.pool, host_id).await {
                return self.caddy.for_host(&h);
            }
        }
        self.caddy.for_region(None)
    }

    async fn agent_client(&self, vm_id: &str) -> anyhow::Result<HostAgentClient<Channel>> {
        let vm = db::get_vm(&self.pool, vm_id)
            .await?
            .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;
        let host_id = vm
            .host_id
            .ok_or_else(|| anyhow!("vm {vm_id} has no host assignment"))?;
        let host = db::get_host(&self.pool, &host_id)
            .await?
            .ok_or_else(|| anyhow!("host {host_id} not found"))?;
        let channel = Channel::from_shared(host.address)?.connect().await?;
        Ok(HostAgentClient::new(channel))
    }
}

#[async_trait]
impl api::VmOps for ControlPlaneOps {
    async fn create_vm(
        &self,
        account_id: String,
        req: api::CreateVmRequest,
    ) -> anyhow::Result<db::VmRow> {
        let account = db::get_account(&self.pool, &account_id)
            .await?
            .ok_or_else(|| anyhow!("account not found: {account_id}"))?;

        let (image_name, image_tag) = match req.image.split_once(':') {
            Some((n, t)) => (n, t),
            None => (req.image.as_str(), "latest"),
        };
        let image = db::get_image_by_name_tag(&self.pool, image_name, image_tag)
            .await?
            .ok_or_else(|| anyhow!("image '{}' not found", req.image))?;
        if image.status != "ready" {
            return Err(anyhow!(
                "image '{}' is not ready (status: {})",
                req.image,
                image.status
            ));
        }

        // Merge an explicit `region` field into required_labels so callers
        // don't have to know the label key name.
        let effective_labels = match (&req.region, &req.required_labels) {
            (Some(r), Some(labels)) => {
                let mut merged = labels.clone();
                if let Some(map) = merged.as_object_mut() {
                    map.insert("region".to_string(), serde_json::Value::String(r.clone()));
                }
                Some(merged)
            }
            (Some(r), None) => Some(serde_json::json!({"region": r})),
            (None, labels) => labels.clone(),
        };

        let host = scheduler::pick_host(
            &self.pool,
            req.vcpus,
            req.memory_mb,
            &req.placement_strategy,
            effective_labels.as_ref(),
        )
        .await?;
        let vm_id = uuid::Uuid::new_v4().to_string();
        let used_ips = db::get_used_ips(&self.pool).await?;
        let slot = scheduler::next_free_slot(&used_ips);
        let ip_address = format!("172.16.{slot}.2");
        let sub = subdomain::generate(&self.pool, &req.name, &account.username).await?;

        let channel = Channel::from_shared(host.address.clone())?
            .connect()
            .await?;
        let mut agent = HostAgentClient::new(channel);

        let resp = agent
            .create_vm(CreateVmRequest {
                vm_id: vm_id.clone(),
                account_id,
                name: req.name,
                subdomain: sub,
                image: image.id,
                vcpus: req.vcpus,
                memory_mb: req.memory_mb,
                disk_mb: req.disk_mb,
                bandwidth_mbps: req.bandwidth_mbps,
                exposed_port: req.exposed_port,
                ip_address,
            })
            .await?
            .into_inner();

        if !resp.ok {
            return Err(anyhow!("agent failed to create vm: {}", resp.error));
        }

        // Persist the region the VM actually landed in (from the placed host's labels).
        if let Some(region) = host
            .labels
            .as_object()
            .and_then(|m| m.get("region"))
            .and_then(|v| v.as_str())
        {
            let _ = db::set_vm_region(&self.pool, &vm_id, region).await;
        }

        let vm = db::get_vm(&self.pool, &vm_id)
            .await?
            .ok_or_else(|| anyhow!("vm {vm_id} not found after creation"))?;

        let _ = self
            .caddy
            .for_host(&host)
            .set_stopped_route(&vm.subdomain)
            .await;
        Ok(vm)
    }

    async fn start_vm(&self, id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, id)
            .await?
            .ok_or_else(|| anyhow!("vm not found: {id}"))?;

        // quota check + atomic status='starting' in a serializable tx
        let result =
            db::check_quota_and_reserve(&self.pool, &vm.account_id, id, vm.vcpus, vm.memory_mb)
                .await;

        match result {
            Ok(()) => {}
            Err(db::QuotaError::Serialization) => {
                // retry once on serialization conflict
                db::check_quota_and_reserve(&self.pool, &vm.account_id, id, vm.vcpus, vm.memory_mb)
                    .await
                    .map_err(|e| anyhow!("{e}"))?;
            }
            Err(e) => return Err(anyhow!("{e}")),
        }

        let mut agent = self.agent_client(id).await?;
        let resp = agent
            .start_vm(StartVmRequest { vm_id: id.into() })
            .await?
            .into_inner();
        if !resp.ok {
            // revert status on agent failure
            let _ = db::set_vm_status(&self.pool, id, "stopped").await;
            return Err(anyhow!("agent start_vm failed: {}", resp.error));
        }
        Ok(())
    }

    async fn stop_vm(&self, id: &str) -> anyhow::Result<()> {
        let mut agent = self.agent_client(id).await?;
        let resp = agent
            .stop_vm(StopVmRequest { vm_id: id.into() })
            .await?
            .into_inner();
        if !resp.ok {
            return Err(anyhow!("agent stop_vm failed: {}", resp.error));
        }
        Ok(())
    }

    async fn delete_vm(&self, id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, id)
            .await?
            .ok_or_else(|| anyhow!("vm not found: {id}"))?;
        if vm.status != "stopped" {
            return Err(anyhow!("vm must be stopped before deletion"));
        }
        let mut agent = self.agent_client(id).await?;
        let resp = agent
            .delete_vm(DeleteVmRequest { vm_id: id.into() })
            .await?
            .into_inner();
        if !resp.ok {
            return Err(anyhow!("agent delete_vm failed: {}", resp.error));
        }
        let caddy = self.caddy_for_vm(&vm).await;
        if let Err(e) = caddy.set_stopped_route(&vm.subdomain).await {
            error!("failed to update caddy route for deleted {id}: {e}");
        }
        Ok(())
    }

    async fn get_vm(&self, id: &str) -> anyhow::Result<Option<db::VmRow>> {
        Ok(db::get_vm(&self.pool, id).await?)
    }

    async fn list_vms(&self, account_id: &str) -> anyhow::Result<Vec<db::VmRow>> {
        Ok(db::list_vms(&self.pool, account_id).await?)
    }

    async fn take_snapshot(
        &self,
        vm_id: &str,
        label: Option<String>,
    ) -> anyhow::Result<db::SnapshotRow> {
        let mut agent = self.agent_client(vm_id).await?;
        let resp = agent
            .take_snapshot(TakeSnapshotRequest {
                vm_id: vm_id.into(),
                label: label.unwrap_or_default(),
            })
            .await?
            .into_inner();

        if !resp.ok {
            return Err(anyhow!("agent take_snapshot failed: {}", resp.error));
        }

        db::get_snapshot(&self.pool, &resp.snap_id)
            .await?
            .ok_or_else(|| anyhow!("snapshot {} not found after creation", resp.snap_id))
    }

    async fn list_snapshots(&self, vm_id: &str) -> anyhow::Result<Vec<db::SnapshotRow>> {
        Ok(db::list_snapshots(&self.pool, vm_id).await?)
    }

    async fn delete_snapshot(&self, _vm_id: &str, snap_id: &str) -> anyhow::Result<()> {
        let snap = db::get_snapshot(&self.pool, snap_id)
            .await?
            .ok_or_else(|| anyhow!("snapshot not found: {snap_id}"))?;
        db::delete_snapshot(&self.pool, snap_id).await?;
        let _ = snap;
        Ok(())
    }

    async fn restore_snapshot(&self, vm_id: &str, snap_id: &str) -> anyhow::Result<()> {
        let mut agent = self.agent_client(vm_id).await?;
        let resp = agent
            .restore_snapshot(RestoreRequest {
                vm_id: vm_id.into(),
                snap_id: snap_id.into(),
            })
            .await?
            .into_inner();

        if !resp.ok {
            return Err(anyhow!("agent restore_snapshot failed: {}", resp.error));
        }
        Ok(())
    }

    async fn update_vm(
        &self,
        vm_id: &str,
        account_id: &str,
        patch: api::VmPatch,
    ) -> anyhow::Result<db::VmRow> {
        if let Some(new_name) = &patch.name {
            let account = db::get_account(&self.pool, account_id)
                .await?
                .ok_or_else(|| anyhow!("account not found: {account_id}"))?;
            let new_subdomain =
                subdomain::generate(&self.pool, new_name, &account.username).await?;

            let current = db::get_vm(&self.pool, vm_id)
                .await?
                .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;
            let old_subdomain = current.subdomain.clone();

            db::rename_vm(&self.pool, vm_id, new_name, &new_subdomain).await?;

            let caddy = self.caddy_for_vm(&current).await;
            let set_result = if current.status == "running" {
                caddy
                    .set_vm_route(
                        &new_subdomain,
                        &current.ip_address,
                        current.exposed_port as u16,
                    )
                    .await
            } else {
                caddy.set_stopped_route(&new_subdomain).await
            };
            if let Err(e) = set_result {
                error!("rename: failed to set caddy route {new_subdomain}: {e}");
            }
            if let Err(e) = caddy.delete_route(&old_subdomain).await {
                error!("rename: failed to delete old caddy route {old_subdomain}: {e}");
            }
        }

        if let Some(port) = patch.exposed_port {
            let current = db::get_vm(&self.pool, vm_id)
                .await?
                .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;
            db::update_vm_port(&self.pool, vm_id, port).await?;
            if current.status == "running" {
                let caddy = self.caddy_for_vm(&current).await;
                if let Err(e) = caddy
                    .set_vm_route(&current.subdomain, &current.ip_address, port as u16)
                    .await
                {
                    error!("update_port: failed to update caddy route for {vm_id}: {e}");
                }
            }
        }

        db::get_vm(&self.pool, vm_id)
            .await?
            .ok_or_else(|| anyhow!("vm {vm_id} not found after update"))
    }

    async fn resize_resources(
        &self,
        vm_id: &str,
        _account_id: &str,
        patch: api::VmResourcePatch,
    ) -> anyhow::Result<db::VmRow> {
        let vm = db::get_vm(&self.pool, vm_id)
            .await?
            .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;

        if patch.memory_mb.is_some_and(|m| m != vm.memory_mb) && vm.status == "running" {
            return Err(anyhow!(
                "restart required: memory changes cannot be applied to a running vm"
            ));
        }

        let new_vcpus = patch.vcpus.unwrap_or(vm.vcpus);
        let new_memory = patch.memory_mb.unwrap_or(vm.memory_mb);
        let new_bandwidth = patch.bandwidth_mbps.unwrap_or(vm.bandwidth_mbps);

        db::update_vm_resources(&self.pool, vm_id, new_vcpus, new_memory, new_bandwidth).await?;

        if vm.status == "running" && patch.vcpus.is_some_and(|v| v != vm.vcpus) {
            let mut agent = self.agent_client(vm_id).await?;
            let resp = agent
                .resize_cpu(ResizeCpuRequest {
                    vm_id: vm_id.into(),
                    vcpus: new_vcpus,
                })
                .await?
                .into_inner();
            if !resp.ok {
                return Err(anyhow!("agent resize_cpu failed: {}", resp.error));
            }
        }

        if vm.status == "running" && patch.bandwidth_mbps.is_some_and(|b| b != vm.bandwidth_mbps) {
            let mut agent = self.agent_client(vm_id).await?;
            let resp = agent
                .resize_bandwidth(ResizeBandwidthRequest {
                    vm_id: vm_id.into(),
                    bandwidth_mbps: new_bandwidth,
                })
                .await?
                .into_inner();
            if !resp.ok {
                return Err(anyhow!("agent resize_bandwidth failed: {}", resp.error));
            }
        }

        db::get_vm(&self.pool, vm_id)
            .await?
            .ok_or_else(|| anyhow!("vm {vm_id} not found after resize"))
    }

    async fn clone_vm(
        &self,
        source_id: &str,
        account_id: &str,
        req: api::CloneVmRequest,
    ) -> anyhow::Result<db::VmRow> {
        let source = db::get_vm(&self.pool, source_id)
            .await?
            .ok_or_else(|| anyhow!("vm not found: {source_id}"))?;

        if source.account_id != account_id {
            return Err(anyhow!("vm not found: {source_id}"));
        }

        let account = db::get_account(&self.pool, account_id)
            .await?
            .ok_or_else(|| anyhow!("account not found: {account_id}"))?;

        let host_id = source
            .host_id
            .as_deref()
            .ok_or_else(|| anyhow!("source vm has no host assignment"))?;
        let host = db::get_host(&self.pool, host_id)
            .await?
            .ok_or_else(|| anyhow!("host {host_id} not found"))?;

        let new_vm_id = uuid::Uuid::new_v4().to_string();
        let used_ips = db::get_used_ips(&self.pool).await?;
        let slot = scheduler::next_free_slot(&used_ips);
        let ip_address = format!("172.16.{slot}.2");
        let name = req.name.clone();
        let sub = subdomain::generate(&self.pool, &name, &account.username).await?;

        let channel = Channel::from_shared(host.address.clone())?
            .connect()
            .await?;
        let mut agent = HostAgentClient::new(channel);

        let resp = agent
            .clone_vm(CloneVmRequest {
                source_vm_id: source_id.into(),
                new_vm_id: new_vm_id.clone(),
                account_id: account_id.into(),
                name,
                subdomain: sub,
                ip_address,
                exposed_port: source.exposed_port,
                include_memory: req.include_memory,
            })
            .await?
            .into_inner();

        if !resp.ok {
            return Err(anyhow!("agent failed to clone vm: {}", resp.error));
        }

        let vm = db::get_vm(&self.pool, &new_vm_id)
            .await?
            .ok_or_else(|| anyhow!("vm {new_vm_id} not found after clone"))?;

        let _ = self
            .caddy
            .for_host(&host)
            .set_stopped_route(&vm.subdomain)
            .await;
        Ok(vm)
    }

    async fn change_username(&self, account_id: &str, new_username: &str) -> anyhow::Result<()> {
        let new_username = new_username.to_lowercase();

        if let Err(msg) = auth::validate_username(&new_username) {
            return Err(anyhow!("invalid username: {msg}"));
        }

        let account = db::get_account(&self.pool, account_id)
            .await?
            .ok_or_else(|| anyhow!("account not found: {account_id}"))?;

        if account.username == new_username {
            return Ok(());
        }

        let renamed = db::update_username(
            &self.pool,
            account_id,
            &db::UsernameUpdate {
                old_username: account.username,
                new_username: new_username.clone(),
            },
        )
        .await?;

        for entry in &renamed {
            let vm = match db::get_vm(&self.pool, &entry.vm_id).await {
                Ok(Some(v)) => v,
                Ok(None) => {
                    warn!(
                        "vm {} not found during caddy resync after username change",
                        entry.vm_id
                    );
                    continue;
                }
                Err(e) => {
                    error!("failed to fetch vm {} for caddy resync: {e}", entry.vm_id);
                    continue;
                }
            };

            let caddy = self.caddy_for_vm(&vm).await;
            let result = if vm.status == "running" {
                caddy
                    .set_vm_route(&entry.new_subdomain, &vm.ip_address, vm.exposed_port as u16)
                    .await
            } else {
                caddy.set_stopped_route(&entry.new_subdomain).await
            };

            if let Err(e) = result {
                error!(
                    "failed to set caddy route for {} after username change: {e}",
                    entry.new_subdomain
                );
            }

            if let Err(e) = caddy.delete_route(&entry.old_subdomain).await {
                error!(
                    "failed to delete old caddy route {} after username change: {e}",
                    entry.old_subdomain
                );
            }
        }

        Ok(())
    }
}
