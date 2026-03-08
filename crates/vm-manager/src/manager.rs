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
        configuration::{InitMethod, VmConfiguration, VmConfigurationData},
        models::{BootSource, Drive, MachineConfiguration, NetworkInterface},
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
use router_sync::CaddyClient;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::{overlay, subdomain};

pub type RunningVm = Vm<UnrestrictedVmmExecutor, DirectProcessSpawner, TokioRuntime>;

pub struct VmManager {
    pub pool: db::PgPool,
    pub networking: NetworkManager,
    pub caddy: CaddyClient,
    pub installation: VmmInstallation,
    pub kernel_path: PathBuf,
    pub images_dir: PathBuf,
    pub overlay_dir: PathBuf,
    running: Mutex<HashMap<String, RunningVm>>,
}

impl VmManager {
    pub fn new(
        pool: db::PgPool,
        networking: NetworkManager,
        caddy: CaddyClient,
        installation: VmmInstallation,
        kernel_path: PathBuf,
        images_dir: PathBuf,
        overlay_dir: PathBuf,
    ) -> Self {
        Self {
            pool,
            networking,
            caddy,
            installation,
            kernel_path,
            images_dir,
            overlay_dir,
            running: Mutex::new(HashMap::new()),
        }
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

    async fn start_vm_inner(&self, vm_id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, vm_id).await?.unwrap();

        let overlay_path = vm.overlay_path.as_deref()
            .ok_or_else(|| anyhow!("vm {vm_id} has no overlay_path — was it created before overlayfs support?"))?;

        // derive tap slot from guest IP: 172.16.N.2 → N
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

        // overlay is the per-VM writable ext4 layer
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

        // find PID by looking for the process with our socket path
        let pid = find_firecracker_pid(&socket_path.to_string_lossy())
            .unwrap_or(0);

        db::set_vm_running(&self.pool, vm_id, pid, &tap.name, &socket_path.to_string_lossy()).await?;
        db::log_event(&self.pool, vm_id, "started", None).await?;

        if let Err(e) = self.caddy.set_vm_route(&vm.subdomain, &vm.ip_address, vm.exposed_port as u16).await {
            error!("failed to set caddy route for {vm_id}: {e}");
        }

        self.running.lock().await.insert(vm_id.to_string(), fc_vm);
        info!("vm {vm_id} started (pid={pid}, tap={}, guest={})", tap.name, tap.guest_ip);
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

        // graceful shutdown via fctools if we have the handle
        let mut running = self.running.lock().await;
        if let Some(mut fc_vm) = running.remove(vm_id) {
            let _ = fc_vm.shutdown([
                VmShutdownAction { method: VmShutdownMethod::CtrlAltDel, timeout: Some(Duration::from_secs(8)), graceful: true },
                VmShutdownAction { method: VmShutdownMethod::Kill, timeout: Some(Duration::from_secs(3)), graceful: false },
            ]).await;
            let _ = fc_vm.cleanup().await;
        } else {
            // no in-memory handle — kill by PID if we have one
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

        if let Err(e) = self.caddy.set_stopped_route(&vm.subdomain).await {
            error!("failed to set stopped caddy route for {vm_id}: {e}");
        }

        info!("vm {vm_id} stopped");
        Ok(())
    }
}

fn ip_to_slot(guest_ip: &str) -> anyhow::Result<u32> {
    // format: 172.16.N.2
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

// ── VmOps trait impl (used by the api crate) ────────────────────────────────

#[async_trait::async_trait]
impl api::VmOps for VmManager {
    async fn create_vm(&self, req: api::CreateVmRequest) -> anyhow::Result<db::VmRow> {
        let id = uuid::Uuid::new_v4().to_string();
        let used_ips = db::get_used_ips(&self.pool).await?;
        let slot = next_free_slot(&used_ips);
        let ip = format!("172.16.{slot}.2");
        let sub = subdomain::generate(&self.pool).await?;

        let rootfs_path = self.images_dir.join(format!("{}.sqfs", req.image));
        if !rootfs_path.exists() {
            return Err(anyhow!(
                "image '{}' not found (expected {})",
                req.image,
                rootfs_path.display()
            ));
        }

        let real_init = read_image_init(&self.images_dir, &req.image);

        let overlay_path = self.overlay_dir.join(format!("{id}.ext4"));
        overlay::provision_overlay(&overlay_path, overlay::DEFAULT_OVERLAY_SIZE_MB)
            .with_context(|| format!("provision overlay for vm {id}"))?;

        db::create_vm(&self.pool, &db::NewVm {
            id: id.clone(),
            account_id: "dev".into(),
            name: req.name,
            subdomain: sub,
            vcores: req.vcores,
            memory_mb: req.memory_mb,
            kernel_path: self.kernel_path.to_string_lossy().into(),
            rootfs_path: rootfs_path.to_string_lossy().into(),
            overlay_path: overlay_path.to_string_lossy().into(),
            real_init,
            ip_address: ip,
            exposed_port: req.exposed_port,
        }).await?;

        // set stopped caddy route immediately so subdomain resolves
        let vm = db::get_vm(&self.pool, &id).await?.unwrap();
        let _ = self.caddy.set_stopped_route(&vm.subdomain).await;

        Ok(vm)
    }

    async fn start_vm(&self, id: &str) -> anyhow::Result<()> {
        self.start_vm(id).await
    }

    async fn stop_vm(&self, id: &str) -> anyhow::Result<()> {
        self.stop_vm(id).await
    }

    async fn delete_vm(&self, id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, id).await?
            .ok_or_else(|| anyhow!("vm not found: {id}"))?;
        if vm.status != "stopped" {
            return Err(anyhow!("vm must be stopped before deletion"));
        }
        db::delete_vm(&self.pool, id).await?;
        if let Some(ref path) = vm.overlay_path {
            overlay::remove_overlay(std::path::Path::new(path));
        }
        Ok(())
    }

    async fn get_vm(&self, id: &str) -> anyhow::Result<Option<db::VmRow>> {
        Ok(db::get_vm(&self.pool, id).await?)
    }

    async fn list_vms(&self, account_id: &str) -> anyhow::Result<Vec<db::VmRow>> {
        Ok(db::list_vms(&self.pool, account_id).await?)
    }
}

/// Reads `{images_dir}/{name}.init` for a custom init path, falling back to `/sbin/init`.
fn read_image_init(images_dir: &std::path::Path, name: &str) -> String {
    let sidecar = images_dir.join(format!("{name}.init"));
    std::fs::read_to_string(&sidecar)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "/sbin/init".into())
}

fn next_free_slot(used_ips: &[String]) -> u32 {
    for n in 1..=65534u32 {
        if !used_ips.iter().any(|ip| ip == &format!("172.16.{n}.2")) {
            return n;
        }
    }
    panic!("IP pool exhausted");
}
