use anyhow::anyhow;
use tonic::transport::Channel;
use tracing::{error, info};

use crate::caddy_router::CaddyRouter;
use agent_proto::agent::{
    MigrateVmRequest, StopVmRequest, TakeSnapshotRequest, host_agent_client::HostAgentClient,
};

use crate::scheduler;

async fn agent_client_for_host(host: &db::HostRow) -> anyhow::Result<HostAgentClient<Channel>> {
    let channel = Channel::from_shared(host.address.clone())?
        .connect()
        .await?;
    Ok(HostAgentClient::new(channel))
}

/// Migrate `vm_id` to `target_host_id`.
///
/// Steps:
/// 1. Stop the VM on the source host.
/// 2. Take a snapshot on the source host.
/// 3. Call MigrateVm on the target host — it pulls and restores the snapshot.
/// 4. Update vm.host_id → target, log the migration record.
/// 5. Rebuild caddy route (VM is stopped after migration; caller starts it).
pub async fn migrate_vm(
    pool: &db::PgPool,
    _caddy: &CaddyRouter,
    vm_id: &str,
    target_host_id: &str,
) -> anyhow::Result<()> {
    let vm = db::get_vm(pool, vm_id)
        .await?
        .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;
    let src_host_id = vm
        .host_id
        .as_deref()
        .ok_or_else(|| anyhow!("vm {vm_id} has no host"))?
        .to_string();

    if src_host_id == target_host_id {
        return Err(anyhow!("source and target host are the same"));
    }

    let src_host = db::get_host(pool, &src_host_id)
        .await?
        .ok_or_else(|| anyhow!("source host {src_host_id} not found"))?;
    let tgt_host = db::get_host(pool, target_host_id)
        .await?
        .ok_or_else(|| anyhow!("target host {target_host_id} not found"))?;

    let migration_id = uuid::Uuid::new_v4().to_string();
    db::create_vm_migration(
        pool,
        &db::NewVmMigration {
            id: migration_id.clone(),
            vm_id: vm_id.to_string(),
            from_host: src_host_id.clone(),
            to_host: target_host_id.to_string(),
        },
    )
    .await?;

    let mut src_agent = agent_client_for_host(&src_host).await?;

    // Stop the VM if running.
    if vm.status == "running" {
        let resp = src_agent
            .stop_vm(StopVmRequest {
                vm_id: vm_id.into(),
            })
            .await?
            .into_inner();
        if !resp.ok {
            let _ = db::update_migration_status(pool, &migration_id, "failed", None).await;
            return Err(anyhow!("stop_vm failed: {}", resp.error));
        }
    }

    // Take a snapshot on the source.
    let snap_resp = src_agent
        .take_snapshot(TakeSnapshotRequest {
            vm_id: vm_id.into(),
            label: "migration".into(),
        })
        .await?
        .into_inner();

    if !snap_resp.ok {
        let _ = db::update_migration_status(pool, &migration_id, "failed", None).await;
        return Err(anyhow!("take_snapshot failed: {}", snap_resp.error));
    }

    let snap_id = snap_resp.snap_id;
    info!("migration {migration_id}: snapshot {snap_id} taken on {src_host_id}");

    // Migrate on target.
    let mut tgt_agent = agent_client_for_host(&tgt_host).await?;
    let resp = tgt_agent
        .migrate_vm(MigrateVmRequest {
            vm_id: vm_id.into(),
            snap_id: snap_id.clone(),
            source_snapshot_url: src_host.snapshot_addr.clone(),
            account_id: vm.account_id.clone(),
            name: vm.name.clone(),
            subdomain: vm.subdomain.clone(),
            vcpus: vm.vcpus,
            memory_mb: vm.memory_mb,
            disk_mb: vm.disk_mb,
            bandwidth_mbps: vm.bandwidth_mbps,
            ip_address: vm.ip_address.clone(),
            exposed_port: vm.exposed_port,
            image: vm.base_image.clone(),
            namespace_id: vm.namespace_id.clone(),
        })
        .await?
        .into_inner();

    if !resp.ok {
        let _ = db::update_migration_status(pool, &migration_id, "failed", None).await;
        return Err(anyhow!("migrate_vm on target failed: {}", resp.error));
    }

    // Update host assignment and region.
    db::set_vm_host(pool, vm_id, target_host_id).await?;
    if let Some(region) = tgt_host
        .labels
        .as_object()
        .and_then(|m| m.get("region"))
        .and_then(|v| v.as_str())
    {
        let _ = db::set_vm_region(pool, vm_id, region).await;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    db::update_migration_status(pool, &migration_id, "completed", Some(now)).await?;

    info!("migration {migration_id}: vm {vm_id} → {target_host_id} complete");
    Ok(())
}

/// Background drain task: periodically checks for draining hosts and migrates
/// their VMs to other active hosts.
pub async fn run_drain_watcher(pool: db::PgPool, caddy: CaddyRouter) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        interval.tick().await;
        if let Err(e) = drain_tick(&pool, &caddy).await {
            error!("drain watcher error: {e}");
        }
    }
}

async fn drain_tick(pool: &db::PgPool, caddy: &CaddyRouter) -> anyhow::Result<()> {
    let all_hosts = db::list_hosts(pool).await?;
    let draining: Vec<_> = all_hosts
        .iter()
        .filter(|h| h.status == "draining")
        .collect();

    if draining.is_empty() {
        return Ok(());
    }

    for host in draining {
        let vms = db::get_vms_by_host(pool, &host.id).await?;
        let active_vms: Vec<_> = vms.into_iter().filter(|v| v.status != "stopped").collect();

        if active_vms.is_empty() {
            // Nothing left — flip to offline.
            db::set_host_status(pool, &host.id, "offline").await?;
            info!("host {} drained → offline", host.id);
            continue;
        }

        for vm in active_vms {
            // Prefer a target in the same region as the VM; fall back to any host if none found.
            let region_labels = vm
                .region
                .as_deref()
                .map(|r| serde_json::json!({"region": r}));
            let target_result = match region_labels.as_ref() {
                Some(labels) => {
                    match scheduler::pick_host(pool, vm.vcpus, vm.memory_mb, "spread", Some(labels)).await {
                        Ok(t) => Ok(t),
                        Err(_) => scheduler::pick_host(pool, vm.vcpus, vm.memory_mb, "spread", None).await,
                    }
                }
                None => scheduler::pick_host(pool, vm.vcpus, vm.memory_mb, "spread", None).await,
            };
            match target_result {
                Ok(target) if target.id != host.id => {
                    if let Err(e) = migrate_vm(pool, caddy, &vm.id, &target.id).await {
                        error!("drain: failed to migrate vm {}: {e}", vm.id);
                    }
                }
                Ok(_) => {
                    error!(
                        "drain: no suitable target host for vm {} (only draining host available)",
                        vm.id
                    );
                }
                Err(e) => {
                    error!("drain: scheduler failed for vm {}: {e}", vm.id);
                }
            }
        }
    }

    Ok(())
}
