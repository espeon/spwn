use std::{path::PathBuf, time::Duration};

use anyhow::{Context, anyhow};
use fctools::{
    process_spawner::DirectProcessSpawner,
    runtime::tokio::TokioRuntime,
    vm::{
        Vm,
        api::VmApi,
        configuration::{VmConfiguration, VmConfigurationData},
        models::{
            BootSource, CreateSnapshot, LoadSnapshot, MachineConfiguration, MemoryBackend,
            MemoryBackendType, NetworkOverride, SnapshotType,
        },
    },
    vmm::{
        arguments::{VmmApiSocket, VmmArguments, jailer::JailerArguments},
        executor::jailed::{FlatVirtualPathResolver, JailedVmmExecutor},
        ownership::VmmOwnershipModel,
        resource::{MovedResourceType, ResourceType, system::ResourceSystem},
    },
};
use tracing::{info, warn};

use super::{VmManager, util};

impl VmManager {
    pub async fn take_snapshot(
        &self,
        vm_id: &str,
        label: Option<String>,
    ) -> anyhow::Result<db::SnapshotRow> {
        let vm = db::get_vm(&self.pool, vm_id)
            .await?
            .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;
        if vm.status != "running" {
            return Err(anyhow!(
                "vm {vm_id} must be running to take a snapshot (status: {})",
                vm.status
            ));
        }

        const MAX_SNAPSHOTS: i64 = 2;
        let count = db::count_snapshots(&self.pool, vm_id).await?;
        if count >= MAX_SNAPSHOTS {
            return Err(anyhow!(
                "snapshot limit reached ({MAX_SNAPSHOTS} max) — delete one first"
            ));
        }

        let snap_id = uuid::Uuid::new_v4().to_string();

        let snap_filename = format!("{snap_id}.snap");
        let mem_filename = format!("{snap_id}.mem");

        let snap_virtual = PathBuf::from(format!("/{snap_filename}"));
        let mem_virtual = PathBuf::from(format!("/{mem_filename}"));

        let jail_root = util::jail_root_path(&self.chroot_base_dir, &self.installation, vm_id);
        let snapshot_path = jail_root.join(&snap_filename);
        let mem_path = jail_root.join(&mem_filename);

        let mut running = self.running.lock().await;
        let result = if let Some(mut fc_vm) = running.remove(vm_id) {
            drop(running);

            let r = async {
                fc_vm.pause().await.map_err(|e| anyhow!("pause VM: {e}"))?;

                let snap_res = fc_vm
                    .get_resource_system_mut()
                    .create_resource(&snap_virtual, ResourceType::Produced)
                    .context("create snapshot resource")?;
                let mem_res = fc_vm
                    .get_resource_system_mut()
                    .create_resource(&mem_virtual, ResourceType::Produced)
                    .context("create mem resource")?;

                fc_vm
                    .create_snapshot(CreateSnapshot {
                        snapshot_type: Some(SnapshotType::Full),
                        snapshot: snap_res,
                        mem_file: mem_res,
                    })
                    .await
                    .map_err(|e| anyhow!("create snapshot: {e}"))?;

                fc_vm
                    .resume()
                    .await
                    .map_err(|e| anyhow!("resume VM: {e}"))?;
                Ok::<(), anyhow::Error>(())
            }
            .await;

            self.running.lock().await.insert(vm_id.to_string(), fc_vm);
            r
        } else {
            drop(running);
            let socket = vm
                .socket_path
                .as_deref()
                .ok_or_else(|| anyhow!("vm {vm_id} has no socket path (was agent restarted?)"))?;
            let snap_str = snap_virtual.to_string_lossy().into_owned();
            let mem_str = mem_virtual.to_string_lossy().into_owned();

            let r = async {
                fc_api_call(
                    socket,
                    "PATCH",
                    "/vm",
                    serde_json::json!({"state": "Paused"}),
                )
                .await
                .context("pause VM")?;
                fc_api_call(
                    socket,
                    "PUT",
                    "/snapshot/create",
                    serde_json::json!({
                        "snapshot_type": "Full",
                        "snapshot_path": snap_str,
                        "mem_file_path": mem_str,
                    }),
                )
                .await
                .context("create snapshot")?;
                Ok::<(), anyhow::Error>(())
            }
            .await;

            let _ = fc_api_call(
                socket,
                "PATCH",
                "/vm",
                serde_json::json!({"state": "Resumed"}),
            )
            .await;
            r
        };

        result?;

        let size_bytes = std::fs::metadata(&snapshot_path)
            .map(|m| m.len())
            .unwrap_or(0)
            + std::fs::metadata(&mem_path).map(|m| m.len()).unwrap_or(0);

        let snap = db::create_snapshot(
            &self.pool,
            &db::NewSnapshot {
                id: snap_id.clone(),
                vm_id: vm_id.to_string(),
                label,
                snapshot_path: snapshot_path.to_string_lossy().into(),
                mem_path: mem_path.to_string_lossy().into(),
                size_bytes: size_bytes as i64,
            },
        )
        .await?;

        db::log_event(&self.pool, vm_id, "snapshot", Some(&snap_id)).await?;
        info!("snapshot {snap_id} taken for vm {vm_id} ({size_bytes} bytes)");

        let _ = self.events.send(super::VmEvent::SnapshotTaken {
            vm_id: vm_id.to_string(),
            snap_id,
        });
        Ok(snap)
    }

    pub async fn restore_snapshot(&self, vm_id: &str, snap_id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, vm_id)
            .await?
            .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;
        if vm.status != "stopped" {
            return Err(anyhow!(
                "vm {vm_id} must be stopped before restore (status: {})",
                vm.status
            ));
        }
        let snap = db::get_snapshot(&self.pool, snap_id)
            .await?
            .ok_or_else(|| anyhow!("snapshot not found: {snap_id}"))?;

        db::set_vm_status(&self.pool, vm_id, "starting").await?;

        if let Err(e) = self.restore_snapshot_inner(&vm, &snap).await {
            db::set_vm_status(&self.pool, vm_id, "error").await.ok();
            return Err(e);
        }
        Ok(())
    }

    pub(crate) async fn restore_snapshot_inner(
        &self,
        vm: &db::VmRow,
        snap: &db::SnapshotRow,
    ) -> anyhow::Result<()> {
        let slot = util::ip_to_slot(&vm.ip_address)?;
        let tap = self
            .networking
            .allocate_tap(slot)
            .context("allocate TAP device")?;
        networking::tap::apply_tc_shaping(&tap.name, vm.bandwidth_mbps as u32)
            .with_context(|| format!("apply tc shaping to {}", tap.name))?;

        let jail_id = util::make_jail_id(&vm.id)?;

        let vmm_args = VmmArguments::new(VmmApiSocket::Enabled(PathBuf::from("fc.sock")));
        let jailer_args = JailerArguments::new(jail_id)
            .chroot_base_dir(&self.chroot_base_dir)
            .exec_in_new_pid_ns()
            .daemonize()
            .cgroup_version(fctools::vmm::arguments::jailer::JailerCgroupVersion::V2)
            .cgroup("cpu.weight", format!("{}", util::cpu_weight(vm.vcpus)))
            .cgroup("cpu.max", util::cpu_max(vm.vcpus))
            .cgroup("memory.max", util::memory_max(vm.memory_mb))
            .cgroup("memory.swap.max", "0");
        let executor = JailedVmmExecutor::new(vmm_args, jailer_args, FlatVirtualPathResolver);

        let ownership = VmmOwnershipModel::Downgraded {
            uid: self.jailer_uid,
            gid: self.jailer_gid,
        };
        let mut resource_system =
            ResourceSystem::new(DirectProcessSpawner, TokioRuntime, ownership);

        let snapshot_res = resource_system
            .create_resource(
                PathBuf::from(&snap.snapshot_path),
                ResourceType::Moved(MovedResourceType::HardLinkedOrCopied),
            )
            .context("register snapshot resource")?;
        let mem_res = resource_system
            .create_resource(
                PathBuf::from(&snap.mem_path),
                ResourceType::Moved(MovedResourceType::HardLinkedOrCopied),
            )
            .context("register mem resource")?;

        let kernel_res = resource_system
            .create_resource(
                &self.kernel_path,
                ResourceType::Moved(MovedResourceType::HardLinkedOrCopied),
            )
            .context("register kernel resource")?;

        resource_system
            .create_resource(
                PathBuf::from(&vm.rootfs_path),
                ResourceType::Moved(MovedResourceType::HardLinkedOrCopied),
            )
            .context("register rootfs resource")?;

        if let Some(ref overlay_path) = vm.overlay_path {
            resource_system
                .create_resource(
                    PathBuf::from(overlay_path),
                    ResourceType::Moved(MovedResourceType::HardLinkedOrCopied),
                )
                .context("register overlay resource")?;
        }

        let load_snapshot = LoadSnapshot {
            track_dirty_pages: Some(true),
            mem_backend: MemoryBackend {
                backend_type: MemoryBackendType::File,
                backend: mem_res,
            },
            snapshot: snapshot_res,
            resume_vm: Some(true),
            network_overrides: vec![NetworkOverride {
                iface_id: "eth0".into(),
                host_dev_name: tap.name.clone(),
            }],
        };

        let config = VmConfiguration::RestoredFromSnapshot {
            load_snapshot,
            data: VmConfigurationData {
                boot_source: BootSource {
                    kernel_image: kernel_res,
                    boot_args: None,
                    initrd: None,
                },
                drives: vec![],
                pmem_devices: vec![],
                machine_configuration: MachineConfiguration {
                    vcpu_count: ((vm.vcpus + 999) / 1000).clamp(1, 255) as u8,
                    mem_size_mib: vm.memory_mb as usize,
                    smt: None,
                    track_dirty_pages: Some(true),
                    huge_pages: None,
                },
                cpu_template: None,
                network_interfaces: vec![],
                balloon_device: None,
                vsock_device: None,
                logger_system: None,
                metrics_system: None,
                memory_hotplug_configuration: None,
                mmds_configuration: None,
                entropy_device: None,
            },
        };

        let mut fc_vm = Vm::prepare(executor, resource_system, self.installation.clone(), config)
            .await
            .map_err(|e| anyhow!("prepare VM from snapshot: {e}"))?;

        fc_vm
            .start(Duration::from_secs(10))
            .await
            .map_err(|e| anyhow!("start restored VM: {e}"))?;

        let socket_path = util::jail_root_path(&self.chroot_base_dir, &self.installation, &vm.id)
            .join("fc.sock");
        let pid = util::read_jailer_pid(&vm.id, &self.chroot_base_dir, &self.installation)
            .await
            .unwrap_or_else(|| {
                warn!(
                    "could not read jailer pid for vm {}, falling back to 0",
                    vm.id
                );
                0
            });

        db::set_vm_running(
            &self.pool,
            &vm.id,
            pid,
            &tap.name,
            &socket_path.to_string_lossy(),
        )
        .await?;
        db::log_event(&self.pool, &vm.id, "restored", Some(&snap.id)).await?;

        self.running.lock().await.insert(vm.id.clone(), fc_vm);
        info!(
            "vm {} restored from snapshot {} (pid={pid}, tap={})",
            vm.id, snap.id, tap.name
        );

        let _ = self.events.send(super::VmEvent::Started {
            vm_id: vm.id.clone(),
        });
        Ok(())
    }

    pub async fn delete_snapshot(&self, snap_id: &str) -> anyhow::Result<()> {
        let snap = db::get_snapshot(&self.pool, snap_id)
            .await?
            .ok_or_else(|| anyhow!("snapshot not found: {snap_id}"))?;
        tokio::fs::remove_file(&snap.snapshot_path).await.ok();
        tokio::fs::remove_file(&snap.mem_path).await.ok();
        db::delete_snapshot(&self.pool, snap_id).await?;
        db::log_event(&self.pool, &snap.vm_id, "snapshot_deleted", Some(snap_id)).await?;
        Ok(())
    }
}

async fn fc_api_call(
    socket_path: &str,
    method: &str,
    route: &str,
    body: serde_json::Value,
) -> anyhow::Result<()> {
    use bytes::Bytes;
    use http::Uri;
    use http_body_util::{BodyExt, Full};
    use hyper::Request;
    use hyper_client_sockets::{connector::UnixConnector, tokio::TokioBackend, uri::UnixUri};
    use hyper_util::client::legacy::Client;
    use hyper_util::rt::TokioExecutor;

    let client = Client::builder(TokioExecutor::new())
        .build::<_, Full<Bytes>>(UnixConnector::<TokioBackend>::new());
    let uri = Uri::unix(socket_path, route).map_err(|e| anyhow!("uri: {e}"))?;
    let body_bytes = Bytes::from(serde_json::to_vec(&body)?);
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Full::new(body_bytes))
        .map_err(|e| anyhow!("build request: {e}"))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| anyhow!("request: {e}"))?;
    let status = resp.status();
    if !status.is_success() && status.as_u16() != 204 {
        let bytes = resp.into_body().collect().await?.to_bytes();
        return Err(anyhow!(
            "firecracker API {} {}: {}",
            method,
            route,
            String::from_utf8_lossy(&bytes)
        ));
    }
    Ok(())
}
