use anyhow::anyhow;
use async_trait::async_trait;
use tonic::transport::Channel;
use tracing::error;

use agent_proto::agent::{
    host_agent_client::HostAgentClient,
    CreateVmRequest, DeleteVmRequest, RestoreRequest,
    StartVmRequest, StopVmRequest, TakeSnapshotRequest,
};
use router_sync::CaddyClient;

use crate::{scheduler, subdomain};

pub struct ControlPlaneOps {
    pub pool: db::PgPool,
    pub caddy: CaddyClient,
}

impl ControlPlaneOps {
    async fn agent_client(&self, vm_id: &str) -> anyhow::Result<HostAgentClient<Channel>> {
        let vm = db::get_vm(&self.pool, vm_id).await?
            .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;
        let host_id = vm.host_id
            .ok_or_else(|| anyhow!("vm {vm_id} has no host assignment"))?;
        let host = db::get_host(&self.pool, &host_id).await?
            .ok_or_else(|| anyhow!("host {host_id} not found"))?;
        let channel = Channel::from_shared(host.address)?
            .connect()
            .await?;
        Ok(HostAgentClient::new(channel))
    }
}

#[async_trait]
impl api::VmOps for ControlPlaneOps {
    async fn create_vm(&self, req: api::CreateVmRequest) -> anyhow::Result<db::VmRow> {
        let host = scheduler::pick_host(&self.pool).await?;
        let vm_id = uuid::Uuid::new_v4().to_string();
        let used_ips = db::get_used_ips(&self.pool).await?;
        let slot = scheduler::next_free_slot(&used_ips);
        let ip_address = format!("172.16.{slot}.2");
        let sub = subdomain::generate(&self.pool).await?;

        let channel = Channel::from_shared(host.address.clone())?
            .connect()
            .await?;
        let mut agent = HostAgentClient::new(channel);

        let resp = agent.create_vm(CreateVmRequest {
            vm_id: vm_id.clone(),
            account_id: "dev".into(),
            name: req.name,
            subdomain: sub,
            image: req.image,
            vcores: req.vcores,
            memory_mb: req.memory_mb,
            exposed_port: req.exposed_port,
            ip_address,
        }).await?.into_inner();

        if !resp.ok {
            return Err(anyhow!("agent failed to create vm: {}", resp.error));
        }

        let vm = db::get_vm(&self.pool, &vm_id).await?
            .ok_or_else(|| anyhow!("vm {vm_id} not found after creation"))?;

        let _ = self.caddy.set_stopped_route(&vm.subdomain).await;
        Ok(vm)
    }

    async fn start_vm(&self, id: &str) -> anyhow::Result<()> {
        let mut agent = self.agent_client(id).await?;
        let resp = agent.start_vm(StartVmRequest { vm_id: id.into() }).await?.into_inner();
        if !resp.ok {
            return Err(anyhow!("agent start_vm failed: {}", resp.error));
        }
        Ok(())
    }

    async fn stop_vm(&self, id: &str) -> anyhow::Result<()> {
        let mut agent = self.agent_client(id).await?;
        let resp = agent.stop_vm(StopVmRequest { vm_id: id.into() }).await?.into_inner();
        if !resp.ok {
            return Err(anyhow!("agent stop_vm failed: {}", resp.error));
        }
        Ok(())
    }

    async fn delete_vm(&self, id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, id).await?
            .ok_or_else(|| anyhow!("vm not found: {id}"))?;
        if vm.status != "stopped" {
            return Err(anyhow!("vm must be stopped before deletion"));
        }
        let mut agent = self.agent_client(id).await?;
        let resp = agent.delete_vm(DeleteVmRequest { vm_id: id.into() }).await?.into_inner();
        if !resp.ok {
            return Err(anyhow!("agent delete_vm failed: {}", resp.error));
        }
        // best-effort: remove caddy route for deleted VM
        if let Err(e) = self.caddy.set_stopped_route(&vm.subdomain).await {
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

    async fn take_snapshot(&self, vm_id: &str, label: Option<String>) -> anyhow::Result<db::SnapshotRow> {
        let mut agent = self.agent_client(vm_id).await?;
        let resp = agent.take_snapshot(TakeSnapshotRequest {
            vm_id: vm_id.into(),
            label: label.unwrap_or_default(),
        }).await?.into_inner();

        if !resp.ok {
            return Err(anyhow!("agent take_snapshot failed: {}", resp.error));
        }

        db::get_snapshot(&self.pool, &resp.snap_id).await?
            .ok_or_else(|| anyhow!("snapshot {} not found after creation", resp.snap_id))
    }

    async fn list_snapshots(&self, vm_id: &str) -> anyhow::Result<Vec<db::SnapshotRow>> {
        Ok(db::list_snapshots(&self.pool, vm_id).await?)
    }

    async fn delete_snapshot(&self, _vm_id: &str, snap_id: &str) -> anyhow::Result<()> {
        // agent deletes files; control plane just removes DB row
        // for now delete the DB row and let files be cleaned up on agent side
        let snap = db::get_snapshot(&self.pool, snap_id).await?
            .ok_or_else(|| anyhow!("snapshot not found: {snap_id}"))?;
        db::delete_snapshot(&self.pool, snap_id).await?;
        // best-effort file cleanup via agent would need a DeleteSnapshot RPC (future)
        let _ = snap;
        Ok(())
    }

    async fn restore_snapshot(&self, vm_id: &str, snap_id: &str) -> anyhow::Result<()> {
        let mut agent = self.agent_client(vm_id).await?;
        let resp = agent.restore_snapshot(RestoreRequest {
            vm_id: vm_id.into(),
            snap_id: snap_id.into(),
        }).await?.into_inner();

        if !resp.ok {
            return Err(anyhow!("agent restore_snapshot failed: {}", resp.error));
        }
        Ok(())
    }
}
