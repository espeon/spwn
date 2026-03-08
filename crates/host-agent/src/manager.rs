use std::{
    collections::HashMap,
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, anyhow};
use fctools::{
    process_spawner::DirectProcessSpawner,
    runtime::tokio::TokioRuntime,
    vm::{
        Vm,
        api::VmApi,
        configuration::{InitMethod, VmConfiguration, VmConfigurationData},
        models::{
            BootSource, CreateSnapshot, Drive, LoadSnapshot, MachineConfiguration,
            MemoryBackend, MemoryBackendType, NetworkInterface, NetworkOverride, SnapshotType,
        },
        shutdown::{VmShutdownAction, VmShutdownMethod},
    },
    vmm::{
        arguments::{VmmApiSocket, VmmArguments},
        executor::unrestricted::UnrestrictedVmmExecutor,
        installation::VmmInstallation,
        ownership::VmmOwnershipModel,
        resource::{MovedResourceType, ResourceType, system::ResourceSystem},
    },
};
use networking::NetworkManager;
use tokio::sync::{Mutex, broadcast};
use tracing::{error, info};

use crate::overlay;

pub type RunningVm = Vm<UnrestrictedVmmExecutor, DirectProcessSpawner, TokioRuntime>;

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
    running: Mutex<HashMap<String, RunningVm>>,
    pub events: broadcast::Sender<VmEvent>,
}

impl VmManager {
    pub fn new(
        pool: db::PgPool,
        networking: NetworkManager,
        installation: VmmInstallation,
        kernel_path: PathBuf,
        images_dir: PathBuf,
        overlay_dir: PathBuf,
        snapshot_dir: PathBuf,
        host_id: String,
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
        vcores: i32,
        memory_mb: i32,
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
        overlay::provision_overlay(&overlay_path, overlay::DEFAULT_OVERLAY_SIZE_MB)
            .with_context(|| format!("provision overlay for vm {vm_id}"))?;

        db::create_vm(&self.pool, &db::NewVm {
            id: vm_id.to_string(),
            account_id: account_id.to_string(),
            name: name.to_string(),
            subdomain: subdomain.to_string(),
            vcores,
            memory_mb,
            kernel_path: self.kernel_path.to_string_lossy().into(),
            rootfs_path: rootfs_path.to_string_lossy().into(),
            overlay_path: overlay_path.to_string_lossy().into(),
            real_init,
            ip_address: ip_address.to_string(),
            exposed_port,
        }).await?;

        db::set_vm_host(&self.pool, vm_id, &self.host_id).await?;
        Ok(())
    }

    pub async fn start_vm(&self, vm_id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, vm_id)
            .await?
            .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;

        if vm.status == "running" || vm.status == "starting" {
            return Err(anyhow!("vm {vm_id} is already {}", vm.status));
        }

        db::set_vm_status(&self.pool, vm_id, "starting").await?;

        if let Err(e) = self.start_vm_inner(vm_id).await {
            db::set_vm_status(&self.pool, vm_id, "error").await.ok();
            return Err(e);
        }
        Ok(())
    }

    pub async fn start_vm_inner(&self, vm_id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, vm_id).await?.unwrap();

        let overlay_path = vm.overlay_path.as_deref()
            .ok_or_else(|| anyhow!("vm {vm_id} has no overlay_path"))?;

        // provision overlay if it was deleted or never created
        let overlay_p = std::path::Path::new(overlay_path);
        if !overlay_p.exists() {
            overlay::provision_overlay(overlay_p, overlay::DEFAULT_OVERLAY_SIZE_MB)
                .with_context(|| format!("provision overlay for vm {vm_id}"))?;
        }

        let slot = ip_to_slot(&vm.ip_address)?;
        let tap = self.networking.allocate_tap(slot)
            .context("allocate TAP device")?;

        let socket_path = PathBuf::from(format!("/tmp/fc-{vm_id}.sock"));
        let vmm_args = VmmArguments::new(VmmApiSocket::Enabled(socket_path.clone()));
        let executor = UnrestrictedVmmExecutor::new(vmm_args);

        let spawner = DirectProcessSpawner;
        let runtime = TokioRuntime;
        let mut resource_system = ResourceSystem::new(spawner, runtime, VmmOwnershipModel::Shared);

        let kernel_res = resource_system
            .create_resource(&self.kernel_path, ResourceType::Moved(MovedResourceType::HardLinkedOrCopied))
            .context("register kernel resource")?;

        let rootfs_res = resource_system
            .create_resource(PathBuf::from(&vm.rootfs_path), ResourceType::Moved(MovedResourceType::HardLinkedOrCopied))
            .context("register rootfs resource")?;

        let overlay_res = resource_system
            .create_resource(PathBuf::from(overlay_path), ResourceType::Moved(MovedResourceType::HardLinkedOrCopied))
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
                    vcpu_count: vm.vcores as u8,
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
            .context("prepare VM")?;

        fc_vm.start(Duration::from_secs(5)).await.context("start VM")?;

        let pid = find_firecracker_pid(&socket_path.to_string_lossy()).unwrap_or(0);

        db::set_vm_running(&self.pool, vm_id, pid, &tap.name, &socket_path.to_string_lossy()).await?;
        db::log_event(&self.pool, vm_id, "started", None).await?;

        self.running.lock().await.insert(vm_id.to_string(), fc_vm);
        info!("vm {vm_id} started (pid={pid}, tap={}, guest={})", tap.name, tap.guest_ip);

        let _ = self.events.send(VmEvent::Started { vm_id: vm_id.to_string() });
        Ok(())
    }

    pub async fn shutdown(&self) {
        let vm_ids: Vec<String> = self.running.lock().await.keys().cloned().collect();
        info!("shutting down {} running vm(s)...", vm_ids.len());
        for vm_id in vm_ids {
            if let Err(e) = self.stop_vm(&vm_id).await {
                error!("failed to stop vm {vm_id} during shutdown: {e}");
            }
        }
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
            let _ = fc_vm.shutdown([
                VmShutdownAction { method: VmShutdownMethod::CtrlAltDel, timeout: Some(Duration::from_secs(8)), graceful: true },
                VmShutdownAction { method: VmShutdownMethod::Kill, timeout: Some(Duration::from_secs(3)), graceful: false },
            ]).await;
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
        let _ = self.events.send(VmEvent::Stopped { vm_id: vm_id.to_string() });
        Ok(())
    }

    pub async fn delete_vm(&self, vm_id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, vm_id).await?
            .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;
        if vm.status != "stopped" {
            return Err(anyhow!("vm must be stopped before deletion"));
        }
        let snaps = db::list_snapshots(&self.pool, vm_id).await?;
        for snap in snaps {
            std::fs::remove_file(&snap.snapshot_path).ok();
            std::fs::remove_file(&snap.mem_path).ok();
        }
        db::delete_vm(&self.pool, vm_id).await?;
        if let Some(ref path) = vm.overlay_path {
            overlay::remove_overlay(std::path::Path::new(path));
        }
        Ok(())
    }

    pub async fn take_snapshot(&self, vm_id: &str, label: Option<String>) -> anyhow::Result<db::SnapshotRow> {
        let vm = db::get_vm(&self.pool, vm_id).await?
            .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;
        if vm.status != "running" {
            return Err(anyhow!("vm {vm_id} must be running to take a snapshot (status: {})", vm.status));
        }

        const MAX_SNAPSHOTS: i64 = 2;
        let count = db::count_snapshots(&self.pool, vm_id).await?;
        if count >= MAX_SNAPSHOTS {
            return Err(anyhow!("snapshot limit reached ({MAX_SNAPSHOTS} max) — delete one first"));
        }

        let snap_id = uuid::Uuid::new_v4().to_string();
        let snap_dir = self.snapshot_dir.join(vm_id);
        std::fs::create_dir_all(&snap_dir)
            .with_context(|| format!("create snapshot dir: {}", snap_dir.display()))?;
        let snapshot_path = snap_dir.join(format!("{snap_id}.snap"));
        let mem_path = snap_dir.join(format!("{snap_id}.mem"));

        let mut running = self.running.lock().await;
        let mut fc_vm = running.remove(vm_id)
            .ok_or_else(|| anyhow!("vm {vm_id} is not in running set"))?;
        drop(running);

        let result = async {
            fc_vm.pause().await.context("pause VM")?;

            let snap_res = fc_vm.get_resource_system_mut()
                .create_resource(&snapshot_path, ResourceType::Produced)
                .context("create snapshot resource")?;
            let mem_res = fc_vm.get_resource_system_mut()
                .create_resource(&mem_path, ResourceType::Produced)
                .context("create mem resource")?;

            fc_vm.create_snapshot(CreateSnapshot {
                snapshot_type: Some(SnapshotType::Full),
                snapshot: snap_res,
                mem_file: mem_res,
            }).await.context("create snapshot")?;

            fc_vm.resume().await.context("resume VM")?;
            Ok::<(), anyhow::Error>(())
        }.await;

        self.running.lock().await.insert(vm_id.to_string(), fc_vm);

        result?;

        let size_bytes = std::fs::metadata(&snapshot_path).map(|m| m.len()).unwrap_or(0)
            + std::fs::metadata(&mem_path).map(|m| m.len()).unwrap_or(0);

        let snap = db::create_snapshot(&self.pool, &db::NewSnapshot {
            id: snap_id.clone(),
            vm_id: vm_id.to_string(),
            label,
            snapshot_path: snapshot_path.to_string_lossy().into(),
            mem_path: mem_path.to_string_lossy().into(),
            size_bytes: size_bytes as i64,
        }).await?;

        db::log_event(&self.pool, vm_id, "snapshot", Some(&snap_id)).await?;
        info!("snapshot {snap_id} taken for vm {vm_id} ({size_bytes} bytes)");

        let _ = self.events.send(VmEvent::SnapshotTaken { vm_id: vm_id.to_string(), snap_id });
        Ok(snap)
    }

    pub async fn restore_snapshot(&self, vm_id: &str, snap_id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, vm_id).await?
            .ok_or_else(|| anyhow!("vm not found: {vm_id}"))?;
        if vm.status != "stopped" {
            return Err(anyhow!("vm {vm_id} must be stopped before restore (status: {})", vm.status));
        }
        let snap = db::get_snapshot(&self.pool, snap_id).await?
            .ok_or_else(|| anyhow!("snapshot not found: {snap_id}"))?;

        db::set_vm_status(&self.pool, vm_id, "starting").await?;

        if let Err(e) = self.restore_snapshot_inner(&vm, &snap).await {
            db::set_vm_status(&self.pool, vm_id, "error").await.ok();
            return Err(e);
        }
        Ok(())
    }

    async fn restore_snapshot_inner(&self, vm: &db::VmRow, snap: &db::SnapshotRow) -> anyhow::Result<()> {
        let slot = ip_to_slot(&vm.ip_address)?;
        let tap = self.networking.allocate_tap(slot).context("allocate TAP device")?;

        let socket_path = PathBuf::from(format!("/tmp/fc-{}.sock", vm.id));
        let vmm_args = VmmArguments::new(VmmApiSocket::Enabled(socket_path.clone()));
        let executor = UnrestrictedVmmExecutor::new(vmm_args);

        let mut resource_system = ResourceSystem::new(DirectProcessSpawner, TokioRuntime, VmmOwnershipModel::Shared);

        let snapshot_res = resource_system
            .create_resource(PathBuf::from(&snap.snapshot_path), ResourceType::Moved(MovedResourceType::HardLinkedOrCopied))
            .context("register snapshot resource")?;
        let mem_res = resource_system
            .create_resource(PathBuf::from(&snap.mem_path), ResourceType::Moved(MovedResourceType::HardLinkedOrCopied))
            .context("register mem resource")?;

        resource_system
            .create_resource(&self.kernel_path, ResourceType::Moved(MovedResourceType::HardLinkedOrCopied))
            .context("register kernel resource")?;
        resource_system
            .create_resource(PathBuf::from(&vm.rootfs_path), ResourceType::Moved(MovedResourceType::HardLinkedOrCopied))
            .context("register rootfs resource")?;
        if let Some(ref overlay_path) = vm.overlay_path {
            resource_system
                .create_resource(PathBuf::from(overlay_path), ResourceType::Moved(MovedResourceType::HardLinkedOrCopied))
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
                    kernel_image: resource_system
                        .create_resource(&self.kernel_path, ResourceType::Moved(MovedResourceType::HardLinkedOrCopied))
                        .unwrap_or_else(|_| unreachable!()),
                    boot_args: None,
                    initrd: None,
                },
                drives: vec![],
                pmem_devices: vec![],
                machine_configuration: MachineConfiguration {
                    vcpu_count: vm.vcores as u8,
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
            .context("prepare VM from snapshot")?;

        fc_vm.start(Duration::from_secs(10)).await.context("start restored VM")?;

        let pid = find_firecracker_pid(&socket_path.to_string_lossy()).unwrap_or(0);

        db::set_vm_running(&self.pool, &vm.id, pid, &tap.name, &socket_path.to_string_lossy()).await?;
        db::log_event(&self.pool, &vm.id, "restored", Some(&snap.id)).await?;

        self.running.lock().await.insert(vm.id.clone(), fc_vm);
        info!("vm {} restored from snapshot {} (pid={pid}, tap={})", vm.id, snap.id, tap.name);

        let _ = self.events.send(VmEvent::Started { vm_id: vm.id.clone() });
        Ok(())
    }
}

fn ip_to_slot(guest_ip: &str) -> anyhow::Result<u32> {
    let parts: Vec<&str> = guest_ip.split('.').collect();
    if parts.len() != 4 {
        return Err(anyhow!("invalid guest IP: {guest_ip}"));
    }
    parts[2].parse::<u32>().context("parse slot from IP")
}

fn find_firecracker_pid(socket_path: &str) -> Option<i64> {
    let proc = std::fs::read_dir("/proc").ok()?;
    for entry in proc.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let cmdline_path = format!("/proc/{name}/cmdline");
        if let Ok(cmdline) = std::fs::read_to_string(&cmdline_path) {
            if cmdline.contains("firecracker") && cmdline.contains(socket_path) {
                return name.parse::<i64>().ok();
            }
        }
    }
    None
}

fn kill_pid(pid: i32) {
    use nix::{sys::signal, unistd::Pid};
    let _ = signal::kill(Pid::from_raw(pid), signal::Signal::SIGKILL);
}

fn read_image_init(images_dir: &std::path::Path, name: &str) -> String {
    let sidecar = images_dir.join(format!("{name}.init"));
    std::fs::read_to_string(&sidecar)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "/sbin/init".into())
}
