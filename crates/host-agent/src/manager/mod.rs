mod clone;
mod snapshot;
pub(crate) mod util;

use std::{collections::HashMap, path::PathBuf, time::Duration};

use anyhow::{Context, anyhow};
use fctools::{
    process_spawner::DirectProcessSpawner,
    runtime::tokio::TokioRuntime,
    vm::{
        Vm,
        configuration::{InitMethod, VmConfiguration, VmConfigurationData},
        models::{BootSource, Drive, MachineConfiguration, NetworkInterface},
        shutdown::{VmShutdownAction, VmShutdownMethod},
    },
    vmm::{
        arguments::{VmmApiSocket, VmmArguments, jailer::JailerArguments},
        executor::jailed::{FlatVirtualPathResolver, JailedVmmExecutor},
        installation::VmmInstallation,
        ownership::VmmOwnershipModel,
        resource::{MovedResourceType, ResourceType, system::ResourceSystem},
    },
};
use networking::NetworkManager;
use tokio::sync::{Mutex, broadcast};
use tracing::{error, info, warn};

use crate::overlay;
use util::{
    cpu_max, cpu_weight, ip_to_slot, kill_pid, make_jail_id, memory_max, persist_overlay_from_jail,
    read_image_init, read_jailer_pid,
};

pub type RunningVm =
    Vm<JailedVmmExecutor<FlatVirtualPathResolver>, DirectProcessSpawner, TokioRuntime>;

#[derive(Debug, Clone)]
pub enum VmEvent {
    Started { vm_id: String },
    Stopped { vm_id: String },
    Crashed { vm_id: String },
    SnapshotTaken { vm_id: String, snap_id: String },
}

pub struct VmManager {
    pub pool: db::PgPool,
    pub networking: NetworkManager,
    pub installation: VmmInstallation,
    pub kernel_path: PathBuf,
    pub images_dir: PathBuf,
    pub overlay_dir: PathBuf,
    pub snapshot_dir: PathBuf,
    pub host_id: String,
    pub jailer_uid: u32,
    pub jailer_gid: u32,
    pub chroot_base_dir: PathBuf,
    pub(crate) running: Mutex<HashMap<String, RunningVm>>,
    pub events: broadcast::Sender<VmEvent>,
}

impl VmManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: db::PgPool,
        networking: NetworkManager,
        installation: VmmInstallation,
        kernel_path: PathBuf,
        images_dir: PathBuf,
        overlay_dir: PathBuf,
        snapshot_dir: PathBuf,
        host_id: String,
        jailer_uid: u32,
        jailer_gid: u32,
        chroot_base_dir: PathBuf,
    ) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            pool,
            networking,
            installation,
            kernel_path,
            images_dir,
            overlay_dir,
            snapshot_dir,
            host_id,
            jailer_uid,
            jailer_gid,
            chroot_base_dir,
            running: Mutex::new(HashMap::new()),
            events,
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<VmEvent> {
        self.events.subscribe()
    }

    pub async fn create_vm(
        &self,
        vm_id: &str,
        account_id: &str,
        name: &str,
        subdomain: &str,
        image: &str,
        vcpus: i64,
        memory_mb: i32,
        disk_mb: i32,
        bandwidth_mbps: i32,
        exposed_port: i32,
        ip_address: &str,
    ) -> anyhow::Result<()> {
        let rootfs_path = self.images_dir.join(format!("{image}.sqfs"));
        if !rootfs_path.exists() {
            return Err(anyhow!(
                "image '{}' not found (expected {})",
                image,
                rootfs_path.display()
            ));
        }

        let real_init = read_image_init(&self.images_dir, image);

        let overlay_path = self.overlay_dir.join(format!("{vm_id}.ext4"));
        overlay::provision_overlay(&overlay_path, disk_mb as u64)
            .with_context(|| format!("provision overlay for vm {vm_id}"))?;

        db::create_vm(
            &self.pool,
            &db::NewVm {
                id: vm_id.to_string(),
                account_id: account_id.to_string(),
                name: name.to_string(),
                subdomain: subdomain.to_string(),
                vcpus,
                memory_mb,
                disk_mb,
                bandwidth_mbps,
                kernel_path: self.kernel_path.to_string_lossy().into(),
                rootfs_path: rootfs_path.to_string_lossy().into(),
                overlay_path: overlay_path.to_string_lossy().into(),
                real_init,
                ip_address: ip_address.to_string(),
                exposed_port,
                base_image: image.to_string(),
                cloned_from: None,
                placement_strategy: "best_fit".into(),
                required_labels: None,
                host_id: Some(self.host_id.clone()),
            },
        )
        .await?;

        let usage = overlay::measure_overlay_usage_mb(&overlay_path);
        db::update_disk_usage_mb(&self.pool, vm_id, usage)
            .await
            .ok();

        Ok(())
    }

    pub async fn start_vm(&self, vm_id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, vm_id)
            .await?
            .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;

        if vm.status == "running" {
            return Err(anyhow!("vm {vm_id} is already running"));
        }

        if let Err(e) = self.start_vm_inner(vm).await {
            db::set_vm_status(&self.pool, vm_id, "error").await.ok();
            return Err(e);
        }
        Ok(())
    }

    async fn start_vm_inner(&self, vm: db::VmRow) -> anyhow::Result<()> {
        let vm_id = vm.id.clone();
        let overlay_path = vm
            .overlay_path
            .as_deref()
            .ok_or_else(|| anyhow!("vm {vm_id} has no overlay_path"))?;

        let overlay_p = std::path::Path::new(overlay_path);
        if !overlay_p.exists() {
            overlay::provision_overlay(overlay_p, vm.disk_mb as u64)
                .with_context(|| format!("provision overlay for vm {vm_id}"))?;
        }

        let slot = ip_to_slot(&vm.ip_address)?;
        let tap = self
            .networking
            .allocate_tap(slot)
            .context("allocate TAP device")?;
        networking::tap::apply_tc_shaping(&tap.name, vm.bandwidth_mbps as u32)
            .with_context(|| format!("apply tc shaping to {}", tap.name))?;

        let jail_id = make_jail_id(&vm_id)?;

        let vmm_args = VmmArguments::new(VmmApiSocket::Enabled(PathBuf::from("fc.sock")));
        let jailer_args = JailerArguments::new(jail_id)
            .chroot_base_dir(&self.chroot_base_dir)
            .exec_in_new_pid_ns()
            .daemonize()
            .cgroup_version(fctools::vmm::arguments::jailer::JailerCgroupVersion::V2)
            .cgroup("cpu.weight", format!("{}", cpu_weight(vm.vcpus)))
            .cgroup("cpu.max", cpu_max(vm.vcpus))
            .cgroup("memory.max", memory_max(vm.memory_mb))
            .cgroup("memory.swap.max", "0");
        let executor = JailedVmmExecutor::new(vmm_args, jailer_args, FlatVirtualPathResolver);

        let ownership = VmmOwnershipModel::Downgraded {
            uid: self.jailer_uid,
            gid: self.jailer_gid,
        };
        let mut resource_system =
            ResourceSystem::new(DirectProcessSpawner, TokioRuntime, ownership);

        let kernel_res = resource_system
            .create_resource(
                &self.kernel_path,
                ResourceType::Moved(MovedResourceType::HardLinkedOrCopied),
            )
            .context("register kernel resource")?;

        let rootfs_res = resource_system
            .create_resource(
                PathBuf::from(&vm.rootfs_path),
                ResourceType::Moved(MovedResourceType::HardLinkedOrCopied),
            )
            .context("register rootfs resource")?;

        let overlay_res = resource_system
            .create_resource(
                PathBuf::from(overlay_path),
                ResourceType::Moved(MovedResourceType::HardLinkedOrCopied),
            )
            .context("register overlay resource")?;

        let mut boot_args = format!(
            "console=ttyS0 reboot=k panic=1 pci=off {} init=/sbin/overlay-init overlay_root=vdb",
            networking::ip::kernel_boot_args(slot)
        );
        if vm.real_init != "/sbin/init" {
            boot_args.push_str(&format!(" real_init={}", vm.real_init));
        }

        let config = VmConfiguration::New {
            init_method: InitMethod::ViaApiCalls,
            data: VmConfigurationData {
                boot_source: BootSource {
                    kernel_image: kernel_res,
                    boot_args: Some(boot_args),
                    initrd: None,
                },
                drives: vec![
                    Drive {
                        drive_id: "rootfs".into(),
                        is_root_device: true,
                        is_read_only: Some(true),
                        block: Some(rootfs_res),
                        cache_type: None,
                        partuuid: None,
                        rate_limiter: None,
                        io_engine: None,
                        socket: None,
                    },
                    Drive {
                        drive_id: "overlayfs".into(),
                        is_root_device: false,
                        is_read_only: Some(false),
                        block: Some(overlay_res),
                        cache_type: None,
                        partuuid: None,
                        rate_limiter: None,
                        io_engine: None,
                        socket: None,
                    },
                ],
                pmem_devices: vec![],
                machine_configuration: MachineConfiguration {
                    vcpu_count: ((vm.vcpus + 999) / 1000).clamp(1, 255) as u8,
                    mem_size_mib: vm.memory_mb as usize,
                    smt: None,
                    track_dirty_pages: Some(true),
                    huge_pages: None,
                },
                cpu_template: None,
                network_interfaces: vec![NetworkInterface {
                    iface_id: "eth0".into(),
                    host_dev_name: tap.name.clone(),
                    guest_mac: None,
                    rx_rate_limiter: None,
                    tx_rate_limiter: None,
                }],
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
            .map_err(|e| anyhow!("prepare VM: {e}"))?;

        fc_vm
            .start(Duration::from_secs(5))
            .await
            .map_err(|e| anyhow!("start VM: {e}"))?;

        let socket_path =
            util::jail_root_path(&self.chroot_base_dir, &self.installation, &vm_id).join("fc.sock");
        let pid = read_jailer_pid(&vm_id, &self.chroot_base_dir, &self.installation)
            .await
            .unwrap_or_else(|| {
                warn!("could not read jailer pid for vm {vm_id}, falling back to 0");
                0
            });

        db::set_vm_running(
            &self.pool,
            &vm_id,
            pid,
            &tap.name,
            &socket_path.to_string_lossy(),
        )
        .await?;
        db::log_event(&self.pool, &vm_id, "started", None).await?;

        self.running.lock().await.insert(vm_id.clone(), fc_vm);
        info!(
            "vm {vm_id} started (pid={pid}, tap={}, guest={})",
            tap.name, tap.guest_ip
        );

        let _ = self.events.send(VmEvent::Started { vm_id });
        Ok(())
    }

    pub async fn shutdown(self: std::sync::Arc<Self>) {
        let vm_ids: Vec<String> = self.running.lock().await.keys().cloned().collect();
        info!("shutting down {} running vm(s)...", vm_ids.len());
        let mut set = tokio::task::JoinSet::new();
        for vm_id in vm_ids {
            let m = std::sync::Arc::clone(&self);
            set.spawn(async move {
                if let Err(e) = m.stop_vm(&vm_id).await {
                    error!("failed to stop vm {vm_id} during shutdown: {e}");
                }
            });
        }
        while set.join_next().await.is_some() {}
        info!("shutdown complete");
    }

    pub async fn stop_vm(&self, vm_id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, vm_id)
            .await?
            .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;

        if vm.status == "stopped" {
            return Ok(());
        }

        db::set_vm_status(&self.pool, vm_id, "stopping").await.ok();

        let mut running = self.running.lock().await;
        if let Some(mut fc_vm) = running.remove(vm_id) {
            let _ = fc_vm
                .shutdown([
                    VmShutdownAction {
                        method: VmShutdownMethod::CtrlAltDel,
                        timeout: Some(Duration::from_secs(8)),
                        graceful: true,
                    },
                    VmShutdownAction {
                        method: VmShutdownMethod::Kill,
                        timeout: Some(Duration::from_secs(3)),
                        graceful: false,
                    },
                ])
                .await;

            if let Some(ref overlay_path) = vm.overlay_path {
                if let Err(e) = persist_overlay_from_jail(
                    &util::jail_root_path(&self.chroot_base_dir, &self.installation, vm_id),
                    std::path::Path::new(overlay_path),
                )
                .await
                {
                    warn!("failed to persist overlay for vm {vm_id}: {e}");
                }
            }

            let _ = fc_vm.cleanup().await;
        } else {
            if let Some(pid) = vm.pid {
                kill_pid(pid as i32);
            }
        }
        drop(running);

        if let Ok(slot) = ip_to_slot(&vm.ip_address) {
            self.networking.release_tap(slot).ok();
        }
        db::set_vm_stopped(&self.pool, vm_id).await?;
        db::log_event(&self.pool, vm_id, "stopped", None).await?;

        info!("vm {vm_id} stopped");
        let _ = self.events.send(VmEvent::Stopped {
            vm_id: vm_id.to_string(),
        });
        Ok(())
    }

    pub async fn delete_vm(&self, vm_id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, vm_id)
            .await?
            .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;
        if vm.status != "stopped" {
            return Err(anyhow!("vm must be stopped before deletion"));
        }
        let snaps = db::list_snapshots(&self.pool, vm_id).await?;
        for snap in snaps {
            tokio::fs::remove_file(&snap.snapshot_path).await.ok();
            tokio::fs::remove_file(&snap.mem_path).await.ok();
        }
        db::delete_vm(&self.pool, vm_id).await?;
        if let Some(ref path) = vm.overlay_path {
            overlay::remove_overlay(std::path::Path::new(path));
        }
        Ok(())
    }

    pub async fn resize_cpu(&self, vm_id: &str, vcpus: i64) -> anyhow::Result<()> {
        let weight = cpu_weight(vcpus);
        let path = format!("/sys/fs/cgroup/firecracker/{vm_id}/cpu.weight");
        tokio::fs::write(&path, format!("{weight}\n"))
            .await
            .with_context(|| format!("write {path}"))?;
        Ok(())
    }

    pub async fn resize_bandwidth(&self, vm_id: &str, bandwidth_mbps: i32) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, vm_id)
            .await?
            .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;

        if vm.status != "running" {
            return Err(anyhow!("vm {vm_id} is not running"));
        }

        let slot = ip_to_slot(&vm.ip_address)?;
        let tap = networking::tap::tap_name(slot);
        networking::tap::apply_tc_shaping(&tap, bandwidth_mbps as u32)
            .with_context(|| format!("apply tc shaping to {tap}"))?;

        Ok(())
    }
}
