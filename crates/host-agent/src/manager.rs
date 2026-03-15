use std::{collections::HashMap, path::PathBuf, time::Duration};

use anyhow::{Context, anyhow};
use fctools::{
    process_spawner::DirectProcessSpawner,
    runtime::tokio::TokioRuntime,
    vm::{
        Vm,
        api::VmApi,
        configuration::{InitMethod, VmConfiguration, VmConfigurationData},
        models::{
            BootSource, CreateSnapshot, Drive, LoadSnapshot, LoggerSystem, MachineConfiguration,
            MemoryBackend, MemoryBackendType, NetworkInterface, NetworkOverride, SnapshotType,
        },
        shutdown::{VmShutdownAction, VmShutdownMethod},
    },
    vmm::{
        arguments::{VmmApiSocket, VmmArguments, VmmLogLevel, jailer::JailerArguments},
        executor::jailed::{FlatVirtualPathResolver, JailedVmmExecutor},
        id::VmmId,
        installation::VmmInstallation,
        ownership::VmmOwnershipModel,
        resource::{CreatedResourceType, MovedResourceType, ResourceType, system::ResourceSystem},
    },
};
use networking::NetworkManager;
use tokio::sync::{Mutex, broadcast};
use tracing::{error, info, warn};

use crate::overlay;

fn dotfiles_service(repo: &str) -> String {
    format!(
        "[Unit]\n\
         Description=Clone dotfiles repository\n\
         ConditionPathExists=!/root/.dotfiles\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         RemainAfterExit=yes\n\
         ExecStart=/usr/bin/git clone --depth 1 {repo} /root/.dotfiles\n\
         ExecStartPost=/bin/sh -c 'cd /root/.dotfiles && [ -f install.sh ] && bash install.sh || true'\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n"
    )
}

fn inject_dotfiles_into_overlay(overlay_path: &str, repo: &str) -> anyhow::Result<()> {
    use std::process::Command;

    let mnt = tempfile::tempdir().context("create temp mount dir")?;
    let mnt_path = mnt.path();

    let mount_status = Command::new("mount")
        .args(["-o", "loop", overlay_path, &mnt_path.to_string_lossy()])
        .status()
        .context("mount overlay ext4")?;
    if !mount_status.success() {
        anyhow::bail!("mount overlay failed for {overlay_path}");
    }

    let result = (|| -> anyhow::Result<()> {
        let systemd_dir = mnt_path.join("upper/etc/systemd/system");
        std::fs::create_dir_all(&systemd_dir).context("create systemd dir in overlay")?;

        let service_path = systemd_dir.join("dotfiles-init.service");
        std::fs::write(&service_path, dotfiles_service(repo))
            .context("write dotfiles-init.service")?;

        let wants_dir = systemd_dir.join("multi-user.target.wants");
        std::fs::create_dir_all(&wants_dir).context("create wants dir in overlay")?;

        let symlink_path = wants_dir.join("dotfiles-init.service");
        if !symlink_path.exists() {
            std::os::unix::fs::symlink("../dotfiles-init.service", &symlink_path)
                .context("symlink dotfiles-init.service into wants")?;
        }

        Ok(())
    })();

    let umount_status = Command::new("umount")
        .arg(&mnt_path.to_string_lossy().as_ref())
        .status()
        .context("umount overlay")?;
    if !umount_status.success() {
        warn!("umount overlay failed for {overlay_path}");
    }

    result
}

pub type RunningVm =
    Vm<JailedVmmExecutor<FlatVirtualPathResolver>, DirectProcessSpawner, TokioRuntime>;

#[derive(Debug, Clone)]
pub enum VmEvent {
    Started { vm_id: String },
    Stopped { vm_id: String },
    Crashed { vm_id: String },
    SnapshotTaken { vm_id: String, snap_id: String },
}

/// Read the last ~2 KB of the firecracker log file for crash diagnostics.
/// Returns None if the file doesn't exist or can't be read.
pub fn read_fc_log_tail(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    if content.is_empty() {
        return None;
    }
    // keep the last 2000 chars to stay within the db metadata field
    let trimmed = if content.len() > 2000 {
        content[content.len() - 2000..].trim_start()
    } else {
        content.trim()
    };
    Some(trimmed.to_string())
}

pub struct VmManager {
    pub pool: db::PgPool,
    pub networking: NetworkManager,
    pub installation: VmmInstallation,
    pub kernel_path: PathBuf,
    pub images_dir: PathBuf,
    pub overlay_dir: PathBuf,
    #[allow(dead_code)]
    pub snapshot_dir: PathBuf,
    pub host_id: String,
    pub jailer_uid: u32,
    pub jailer_gid: u32,
    pub chroot_base_dir: PathBuf,
    running: Mutex<HashMap<String, RunningVm>>,
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
        namespace_id: &str,
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
                region: None,
                namespace_id: namespace_id.to_string(),
            },
        )
        .await?;

        db::set_vm_host(&self.pool, vm_id, &self.host_id).await?;

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

        if let Err(e) = self.start_vm_inner(vm_id).await {
            db::set_vm_status(&self.pool, vm_id, "error").await.ok();
            db::log_event(&self.pool, vm_id, "error", Some(&format!("{e:#}")))
                .await
                .ok();
            return Err(e);
        }
        Ok(())
    }

    pub async fn start_vm_inner(&self, vm_id: &str) -> anyhow::Result<()> {
        let vm = db::get_vm(&self.pool, vm_id).await?.unwrap();

        let overlay_path = vm
            .overlay_path
            .as_deref()
            .ok_or_else(|| anyhow!("vm {vm_id} has no overlay_path"))?;

        let overlay_p = std::path::Path::new(overlay_path);
        if !overlay_p.exists() {
            overlay::provision_overlay(overlay_p, vm.disk_mb as u64)
                .with_context(|| format!("provision overlay for vm {vm_id}"))?;
        }

        if let Ok(Some(account)) = db::get_account(&self.pool, &vm.account_id).await {
            if let Some(repo) = account.dotfiles_repo.filter(|r| !r.is_empty()) {
                if let Err(e) = inject_dotfiles_into_overlay(overlay_path, &repo) {
                    warn!("dotfiles injection failed for vm {vm_id}: {e:#}");
                }
            }
        }

        let slot = ip_to_slot(&vm.ip_address)?;
        let tap = self
            .networking
            .allocate_tap(slot)
            .context("allocate TAP device")?;
        networking::tap::apply_tc_shaping(&tap.name, vm.bandwidth_mbps as u32)
            .with_context(|| format!("apply tc shaping to {}", tap.name))?;

        let jail_id = make_jail_id(vm_id)?;

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

        let overlay_init_path = read_overlay_init_path(&self.images_dir, &vm.base_image);
        let mut boot_args = format!(
            "console=ttyS0 reboot=k panic=1 pci=off selinux=0 {} init={overlay_init_path} overlay_root=vdb",
            networking::ip::kernel_boot_args(slot)
        );
        if vm.real_init != "/sbin/init" {
            boot_args.push_str(&format!(" real_init={}", vm.real_init));
        }
        let hostname: String = vm
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_lowercase()
            .chars()
            .take(63)
            .collect();
        if !hostname.is_empty() {
            boot_args.push_str(&format!(" hostname={hostname}"));
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
                logger_system: {
                    let log_res = resource_system
                        .create_resource(
                            PathBuf::from("firecracker.log"),
                            ResourceType::Created(CreatedResourceType::File),
                        )
                        .context("register log resource")?;
                    Some(LoggerSystem {
                        logs: Some(log_res),
                        level: Some(VmmLogLevel::Warn),
                        show_level: Some(true),
                        show_log_origin: None,
                        module: None,
                    })
                },
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

        let socket_path = self.jail_socket_path(vm_id);
        let pid = read_jailer_pid(vm_id, &self.chroot_base_dir, &self.installation).unwrap_or_else(
            || {
                warn!("could not read jailer pid for vm {vm_id}, falling back to 0");
                0
            },
        );

        db::set_vm_running(
            &self.pool,
            vm_id,
            pid,
            &tap.name,
            &socket_path.to_string_lossy(),
        )
        .await?;
        db::log_event(&self.pool, vm_id, "started", None).await?;

        self.running.lock().await.insert(vm_id.to_string(), fc_vm);
        info!(
            "vm {vm_id} started (pid={pid}, tap={}, guest={})",
            tap.name, tap.guest_ip
        );

        let _ = self.events.send(VmEvent::Started {
            vm_id: vm_id.to_string(),
        });
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
                    &self.jail_root_path(vm_id),
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
        if !matches!(vm.status.as_str(), "stopped" | "error" | "crashed") {
            return Err(anyhow!(
                "vm must be stopped before deletion (current status: {})",
                vm.status
            ));
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

        // Snapshots are produced inside the chroot. We use Produced resources so
        // fctools writes them at the jail-relative path; we then record the
        // host-side effective path (chroot_base/firecracker/<vm_id>/root/<file>)
        // in the DB so they survive process restarts.
        let snap_filename = format!("{snap_id}.snap");
        let mem_filename = format!("{snap_id}.mem");

        // Virtual (inside-jail) paths for the snapshot files
        let snap_virtual = PathBuf::from(format!("/{snap_filename}"));
        let mem_virtual = PathBuf::from(format!("/{mem_filename}"));

        // Host-side effective paths after the jailer expands the chroot
        let jail_root = self.jail_root_path(vm_id);
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
            // Fallback: VM not in running set (agent was restarted). Drive the
            // firecracker API directly over the unix socket.
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

        let _ = self.events.send(VmEvent::SnapshotTaken {
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

    async fn restore_snapshot_inner(
        &self,
        vm: &db::VmRow,
        snap: &db::SnapshotRow,
    ) -> anyhow::Result<()> {
        let slot = ip_to_slot(&vm.ip_address)?;
        let tap = self
            .networking
            .allocate_tap(slot)
            .context("allocate TAP device")?;
        networking::tap::apply_tc_shaping(&tap.name, vm.bandwidth_mbps as u32)
            .with_context(|| format!("apply tc shaping to {}", tap.name))?;

        let jail_id = make_jail_id(&vm.id)?;

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

        // Snapshot files already live on the host at their recorded paths.
        // Register them as Moved resources so fctools hard-links/copies them
        // into the jail root.
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

        let socket_path = self.jail_socket_path(&vm.id);
        let pid = read_jailer_pid(&vm.id, &self.chroot_base_dir, &self.installation)
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

        let _ = self.events.send(VmEvent::Started {
            vm_id: vm.id.clone(),
        });
        Ok(())
    }

    pub async fn clone_vm(
        &self,
        source_vm_id: &str,
        new_vm_id: &str,
        account_id: &str,
        name: &str,
        subdomain: &str,
        ip_address: &str,
        exposed_port: i32,
        include_memory: bool,
    ) -> anyhow::Result<()> {
        let source = db::get_vm(&self.pool, source_vm_id)
            .await?
            .ok_or_else(|| anyhow!("source vm not found: {source_vm_id}"))?;

        if matches!(
            source.status.as_str(),
            "starting" | "stopping" | "snapshotting"
        ) {
            return Err(anyhow!(
                "source vm is in transitional state: {}",
                source.status
            ));
        }
        if include_memory && source.status != "running" {
            return Err(anyhow!(
                "include_memory requires source vm to be running (status: {})",
                source.status
            ));
        }

        // 1. If include_memory, take a snapshot of the source VM first.
        //    Note: the clone's guest kernel retains the source's IP/ARP state;
        //    users may need to reconfigure networking inside the guest.
        let source_snap = if include_memory {
            Some(
                self.take_snapshot(source_vm_id, Some(format!("clone-source-{new_vm_id}")))
                    .await
                    .context("take snapshot of source before clone")?,
            )
        } else {
            None
        };

        // 2. Copy the source overlay to the new VM's overlay path.
        let source_overlay = source
            .overlay_path
            .as_deref()
            .ok_or_else(|| anyhow!("source vm {source_vm_id} has no overlay"))?;
        let new_overlay_path = self.overlay_dir.join(format!("{new_vm_id}.ext4"));
        copy_sparse(source_overlay, &new_overlay_path)
            .await
            .context("copy overlay")?;

        // 3. Create the new VM DB record, sharing rootfs/real_init with the source.
        db::create_vm(
            &self.pool,
            &db::NewVm {
                id: new_vm_id.to_string(),
                account_id: account_id.to_string(),
                name: name.to_string(),
                subdomain: subdomain.to_string(),
                vcpus: source.vcpus,
                memory_mb: source.memory_mb,
                disk_mb: source.disk_mb,
                bandwidth_mbps: source.bandwidth_mbps,
                kernel_path: self.kernel_path.to_string_lossy().into(),
                rootfs_path: source.rootfs_path.clone(),
                overlay_path: new_overlay_path.to_string_lossy().into(),
                real_init: source.real_init.clone(),
                ip_address: ip_address.to_string(),
                exposed_port,
                base_image: source.base_image.clone(),
                cloned_from: Some(source_vm_id.to_string()),
                placement_strategy: source.placement_strategy.clone(),
                required_labels: source.required_labels.clone(),
                region: source.region.clone(),
                namespace_id: source.namespace_id.clone(),
            },
        )
        .await?;
        db::set_vm_host(&self.pool, new_vm_id, &self.host_id).await?;

        let usage = overlay::measure_overlay_usage_mb(&new_overlay_path);
        db::update_disk_usage_mb(&self.pool, new_vm_id, usage)
            .await
            .ok();

        // 4. If include_memory, copy the snapshot files and restore the clone.
        if let Some(snap) = source_snap {
            let new_snap_id = uuid::Uuid::new_v4().to_string();
            let new_snap_path = self.snapshot_dir.join(format!("{new_snap_id}.snap"));
            let new_mem_path = self.snapshot_dir.join(format!("{new_snap_id}.mem"));

            tokio::fs::copy(&snap.snapshot_path, &new_snap_path)
                .await
                .context("copy snapshot file")?;
            tokio::fs::copy(&snap.mem_path, &new_mem_path)
                .await
                .context("copy mem file")?;

            let size_bytes = new_snap_path.metadata().map(|m| m.len()).unwrap_or(0)
                + new_mem_path.metadata().map(|m| m.len()).unwrap_or(0);

            db::create_snapshot(
                &self.pool,
                &db::NewSnapshot {
                    id: new_snap_id.clone(),
                    vm_id: new_vm_id.to_string(),
                    label: Some("cloned".into()),
                    snapshot_path: new_snap_path.to_string_lossy().into(),
                    mem_path: new_mem_path.to_string_lossy().into(),
                    size_bytes: size_bytes as i64,
                },
            )
            .await?;

            self.restore_snapshot(new_vm_id, &new_snap_id).await?;
        }

        info!("cloned vm {source_vm_id} → {new_vm_id} (include_memory={include_memory})");
        Ok(())
    }

    /// Migrate a VM to this host from another agent.
    ///
    /// Downloads the overlay file from the source agent's snapshot HTTP server,
    /// then creates the VM record pointing to the local overlay copy.
    /// The VM starts in `stopped` state; the caller can start it afterwards.
    pub async fn migrate_vm(
        &self,
        vm_id: &str,
        source_snapshot_url: &str,
        account_id: &str,
        name: &str,
        subdomain: &str,
        vcpus: i64,
        memory_mb: i32,
        disk_mb: i32,
        bandwidth_mbps: i32,
        ip_address: &str,
        exposed_port: i32,
        image: &str,
        agent_secret: &str,
        namespace_id: &str,
    ) -> anyhow::Result<()> {
        let rootfs_path = self.images_dir.join(format!("{image}.sqfs"));
        if !rootfs_path.exists() {
            return Err(anyhow!(
                "image '{image}' not found on target host (expected {})",
                rootfs_path.display()
            ));
        }

        let real_init = read_image_init(&self.images_dir, image);

        let local_overlay = self.overlay_dir.join(format!("{vm_id}.ext4"));
        download_file(
            &format!("{source_snapshot_url}/overlay/{vm_id}"),
            &local_overlay,
            agent_secret,
        )
        .await
        .context("download overlay from source agent")?;

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
                overlay_path: local_overlay.to_string_lossy().into(),
                real_init,
                ip_address: ip_address.to_string(),
                exposed_port,
                base_image: image.to_string(),
                cloned_from: None,
                placement_strategy: "best_fit".into(),
                required_labels: None,
                region: None,
                namespace_id: namespace_id.to_string(),
            },
        )
        .await?;

        db::set_vm_host(&self.pool, vm_id, &self.host_id).await?;

        let usage = overlay::measure_overlay_usage_mb(&local_overlay);
        db::update_disk_usage_mb(&self.pool, vm_id, usage)
            .await
            .ok();

        info!("migrated vm {vm_id} to this host from {source_snapshot_url}");
        Ok(())
    }

    fn jail_root_path(&self, vm_id: &str) -> PathBuf {
        jail_root_path(&self.chroot_base_dir, &self.installation, vm_id)
    }

    pub fn jail_log_path(&self, vm_id: &str) -> PathBuf {
        self.jail_root_path(vm_id).join("firecracker.log")
    }

    fn jail_socket_path(&self, vm_id: &str) -> PathBuf {
        self.jail_root_path(vm_id).join("fc.sock")
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

// Returns the host-side path to the jail root for a given vm.
// Layout: <chroot_base>/<fc_binary_name>/<vm_id>/root
fn jail_root_path(
    chroot_base_dir: &std::path::Path,
    installation: &VmmInstallation,
    vm_id: &str,
) -> PathBuf {
    let fc_name = installation
        .get_firecracker_path()
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("firecracker");
    chroot_base_dir.join(fc_name).join(vm_id).join("root")
}

fn cpu_weight(millis: i64) -> u64 {
    (millis / 10).clamp(1, 10000) as u64
}

// CFS period in microseconds (100ms — the kernel default).
const CPU_PERIOD_US: i64 = 100_000;

// Produces a `cpu.max` value for cgroupv2.
// Format: "<quota_us> <period_us>"
// quota = millis * period / 1000, clamped to at least 1000µs so the kernel
// never rejects the value as too small.
fn cpu_max(millis: i64) -> String {
    let quota = (millis * CPU_PERIOD_US / 1000).max(1000);
    format!("{quota} {CPU_PERIOD_US}")
}

fn memory_max(memory_mb: i32) -> String {
    ((memory_mb as i64) * 1024 * 1024).max(0).to_string()
}

fn ip_to_slot(guest_ip: &str) -> anyhow::Result<u32> {
    let parts: Vec<&str> = guest_ip.split('.').collect();
    if parts.len() != 4 {
        return Err(anyhow!("invalid guest IP: {guest_ip}"));
    }
    parts[2].parse::<u32>().context("parse slot from IP")
}

// Jail IDs must be 5-60 chars, alphanumeric + dashes only.
// UUIDs are 36 chars and only contain hex digits and dashes — valid as-is.
fn make_jail_id(vm_id: &str) -> anyhow::Result<VmmId> {
    VmmId::new(vm_id).map_err(|e| anyhow!("invalid jail id for vm {vm_id}: {e}"))
}

// After exec_in_new_pid_ns the jailer writes a <binary>.pid file in the jail
// root. We poll for it briefly since the jailer may not have flushed it yet.
fn read_jailer_pid(
    vm_id: &str,
    chroot_base_dir: &std::path::Path,
    installation: &VmmInstallation,
) -> Option<i64> {
    let fc_name = installation
        .get_firecracker_path()
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("firecracker");
    let jail_root = jail_root_path(chroot_base_dir, installation, vm_id);
    let pid_file = jail_root.join(format!("{fc_name}.pid"));

    for _ in 0..20 {
        if let Ok(contents) = std::fs::read_to_string(&pid_file) {
            if let Ok(pid) = contents.trim().parse::<i64>() {
                return Some(pid);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    // Fall back to reading the cgroup if the pid file isn't there yet
    read_pid_from_cgroup(vm_id)
}

fn read_pid_from_cgroup(vm_id: &str) -> Option<i64> {
    let cgroup_procs = format!("/sys/fs/cgroup/firecracker/{vm_id}/cgroup.procs");
    let contents = std::fs::read_to_string(&cgroup_procs).ok()?;
    contents.lines().next()?.trim().parse::<i64>().ok()
}

fn kill_pid(pid: i32) {
    use nix::{sys::signal, unistd::Pid};
    let _ = signal::kill(Pid::from_raw(pid), signal::Signal::SIGKILL);
}

async fn copy_sparse(src: &str, dst: &std::path::Path) -> anyhow::Result<()> {
    if let Some(parent) = dst.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let status = tokio::process::Command::new("cp")
        .args(["--sparse=always", src, &dst.to_string_lossy()])
        .status()
        .await
        .context("run cp --sparse=always")?;
    if !status.success() {
        anyhow::bail!("cp --sparse=always failed: {src} -> {}", dst.display());
    }
    Ok(())
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

// Copy the overlay ext4 from the jailer chroot back to the canonical overlay
// path so that changes made inside the VM survive across stop/start cycles and
// are visible to clone operations.
//
// fctools uses HardLinkedOrCopied when moving resources into the jail. On the
// same filesystem the file is hard-linked (same inode, writes are shared
// automatically). On different filesystems it falls back to a full copy, making
// the jail file the only place where writes land. This function handles the
// cross-filesystem case by copying the jail file back before cleanup removes it.
async fn persist_overlay_from_jail(
    jail_root: &std::path::Path,
    canonical: &std::path::Path,
) -> anyhow::Result<()> {
    let filename = canonical
        .file_name()
        .ok_or_else(|| anyhow!("overlay path has no filename"))?;
    let jail_overlay = jail_root.join(filename);

    if !jail_overlay.exists() {
        return Ok(());
    }

    let jail_meta = std::fs::metadata(&jail_overlay).context("stat jail overlay")?;
    let canon_meta = std::fs::metadata(canonical).context("stat canonical overlay")?;

    use std::os::unix::fs::MetadataExt;
    if jail_meta.ino() == canon_meta.ino() && jail_meta.dev() == canon_meta.dev() {
        // Same inode — hard-linked, already in sync.
        return Ok(());
    }

    tokio::fs::copy(&jail_overlay, canonical)
        .await
        .with_context(|| {
            format!(
                "copy overlay {} -> {}",
                jail_overlay.display(),
                canonical.display()
            )
        })?;

    Ok(())
}

fn read_image_init(images_dir: &std::path::Path, name: &str) -> String {
    let sidecar = images_dir.join(format!("{name}.init"));
    std::fs::read_to_string(&sidecar)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "/sbin/init".into())
}

/// Read the overlay-init guest path from the .init sidecar written at build
/// time. Defaults to /sbin/overlay-init if no sidecar exists (pre-usrmerge
/// images or images built by the bash script before this fix).
fn read_overlay_init_path(images_dir: &std::path::Path, image_id: &str) -> String {
    let sidecar = images_dir.join(format!("{image_id}.overlay-init"));
    std::fs::read_to_string(&sidecar)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "/sbin/overlay-init".into())
}

/// Download a remote file to `dest` using a bearer token for auth.
async fn download_file(
    url: &str,
    dest: &std::path::Path,
    bearer_token: &str,
) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let resp = reqwest::Client::new()
        .get(url)
        .bearer_auth(bearer_token)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;

    if !resp.status().is_success() {
        return Err(anyhow!("GET {url}: HTTP {}", resp.status()));
    }

    let body = resp
        .bytes()
        .await
        .with_context(|| format!("read body from {url}"))?;

    tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("create {}", dest.display()))?
        .write_all(&body)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fctools::vmm::installation::VmmInstallation;

    use super::{
        cpu_max, cpu_weight, ip_to_slot, jail_root_path, make_jail_id, memory_max, read_image_init,
        read_jailer_pid,
    };

    fn installation(fc_bin: &str) -> VmmInstallation {
        VmmInstallation::new(
            PathBuf::from(fc_bin),
            PathBuf::from("/usr/local/bin/jailer"),
            PathBuf::from("/usr/local/bin/snapshot-editor"),
        )
    }

    // ── ip_to_slot ────────────────────────────────────────────────────────────

    #[test]
    fn ip_to_slot_extracts_third_octet() {
        assert_eq!(ip_to_slot("172.16.1.2").unwrap(), 1);
        assert_eq!(ip_to_slot("172.16.42.2").unwrap(), 42);
        assert_eq!(ip_to_slot("172.16.255.2").unwrap(), 255);
    }

    #[test]
    fn ip_to_slot_rejects_too_few_octets() {
        assert!(ip_to_slot("172.16.1").is_err());
        assert!(ip_to_slot("").is_err());
    }

    #[test]
    fn ip_to_slot_rejects_non_numeric_octet() {
        assert!(ip_to_slot("172.16.abc.2").is_err());
    }

    // ── make_jail_id ──────────────────────────────────────────────────────────

    #[test]
    fn make_jail_id_accepts_uuid() {
        let id = make_jail_id("550e8400-e29b-41d4-a716-446655440000");
        assert!(id.is_ok(), "uuid should be a valid jail id");
    }

    #[test]
    fn make_jail_id_rejects_too_short() {
        assert!(make_jail_id("ab").is_err());
        assert!(make_jail_id("1234").is_err());
    }

    #[test]
    fn make_jail_id_rejects_invalid_chars() {
        assert!(make_jail_id("invalid_vm_id").is_err());
    }

    #[test]
    fn make_jail_id_accepts_alphanumeric_dashes() {
        assert!(make_jail_id("vm-abc-123").is_ok());
    }

    // ── jail_root_path ────────────────────────────────────────────────────────

    #[test]
    fn jail_root_path_uses_fc_binary_name() {
        let inst = installation("/usr/local/bin/firecracker");
        let vm_id = "550e8400-e29b-41d4-a716-446655440000";
        let root = jail_root_path(std::path::Path::new("/srv/jailer"), &inst, vm_id);
        assert_eq!(
            root,
            PathBuf::from(format!("/srv/jailer/firecracker/{vm_id}/root"))
        );
    }

    #[test]
    fn jail_root_path_uses_custom_fc_binary_name() {
        let inst = installation("/opt/bin/firecracker-1.9");
        let root = jail_root_path(std::path::Path::new("/srv/jailer"), &inst, "vm-test-id-one");
        assert_eq!(
            root,
            PathBuf::from("/srv/jailer/firecracker-1.9/vm-test-id-one/root")
        );
    }

    #[test]
    fn jail_root_path_custom_chroot_base() {
        let inst = installation("/usr/local/bin/firecracker");
        let root = jail_root_path(
            std::path::Path::new("/var/run/jails"),
            &inst,
            "vm-abc-12345",
        );
        assert_eq!(
            root,
            PathBuf::from("/var/run/jails/firecracker/vm-abc-12345/root")
        );
    }

    #[test]
    fn jail_socket_path_is_fc_sock_inside_jail_root() {
        let inst = installation("/usr/local/bin/firecracker");
        let vm_id = "550e8400-e29b-41d4-a716-446655440000";
        let sock =
            jail_root_path(std::path::Path::new("/srv/jailer"), &inst, vm_id).join("fc.sock");
        assert_eq!(
            sock,
            PathBuf::from(format!("/srv/jailer/firecracker/{vm_id}/root/fc.sock"))
        );
    }

    // ── read_jailer_pid ───────────────────────────────────────────────────────

    #[test]
    fn read_jailer_pid_reads_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let vm_id = "vm-pid-test-abcd";
        let inst = installation(dir.path().join("firecracker").to_str().unwrap());

        let jail_root = dir.path().join("firecracker").join(vm_id).join("root");
        std::fs::create_dir_all(&jail_root).unwrap();
        std::fs::write(jail_root.join("firecracker.pid"), "12345\n").unwrap();

        let pid = read_jailer_pid(vm_id, dir.path(), &inst);
        assert_eq!(pid, Some(12345));
    }

    #[test]
    fn read_jailer_pid_trims_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let vm_id = "vm-pid-trim-abcd";
        let inst = installation(dir.path().join("firecracker").to_str().unwrap());

        let jail_root = dir.path().join("firecracker").join(vm_id).join("root");
        std::fs::create_dir_all(&jail_root).unwrap();
        std::fs::write(jail_root.join("firecracker.pid"), "  99\n  ").unwrap();

        let pid = read_jailer_pid(vm_id, dir.path(), &inst);
        assert_eq!(pid, Some(99));
    }

    #[test]
    fn read_jailer_pid_returns_none_when_no_pid_file_and_no_cgroup() {
        let dir = tempfile::tempdir().unwrap();
        let vm_id = "vm-pid-missing-abcd";
        let inst = installation(dir.path().join("firecracker").to_str().unwrap());

        let pid = read_jailer_pid(vm_id, dir.path(), &inst);
        assert_eq!(pid, None);
    }

    #[test]
    fn read_jailer_pid_ignores_non_numeric_content() {
        let dir = tempfile::tempdir().unwrap();
        let vm_id = "vm-pid-bogus-abcd";
        let inst = installation(dir.path().join("firecracker").to_str().unwrap());

        let jail_root = dir.path().join("firecracker").join(vm_id).join("root");
        std::fs::create_dir_all(&jail_root).unwrap();
        std::fs::write(jail_root.join("firecracker.pid"), "not-a-pid\n").unwrap();

        let pid = read_jailer_pid(vm_id, dir.path(), &inst);
        assert_eq!(pid, None);
    }

    // ── read_image_init ───────────────────────────────────────────────────────

    // ── cpu_weight / cpu_max ──────────────────────────────────────────────────

    #[test]
    fn cpu_weight_one_full_core() {
        assert_eq!(cpu_weight(1000), 100);
    }

    #[test]
    fn cpu_weight_half_core() {
        assert_eq!(cpu_weight(500), 50);
    }

    #[test]
    fn cpu_weight_two_cores() {
        assert_eq!(cpu_weight(2000), 200);
    }

    #[test]
    fn cpu_weight_clamps_to_minimum() {
        assert_eq!(cpu_weight(0), 1);
        assert_eq!(cpu_weight(5), 1);
    }

    #[test]
    fn cpu_weight_clamps_to_maximum() {
        assert_eq!(cpu_weight(200_000), 10000);
    }

    // ── memory_max ────────────────────────────────────────────────────────────

    #[test]
    fn memory_max_512mb() {
        assert_eq!(memory_max(512), format!("{}", 512 * 1024 * 1024));
    }

    #[test]
    fn memory_max_1gb() {
        assert_eq!(memory_max(1024), format!("{}", 1024 * 1024 * 1024));
    }

    #[test]
    fn memory_max_zero_clamps() {
        assert_eq!(memory_max(0), "0");
    }

    #[test]
    fn cpu_max_one_full_core() {
        // 1000 millicpus → 100ms quota per 100ms period = 1 full core
        assert_eq!(cpu_max(1000), "100000 100000");
    }

    #[test]
    fn cpu_max_half_core() {
        assert_eq!(cpu_max(500), "50000 100000");
    }

    #[test]
    fn cpu_max_two_cores() {
        // 2000 millicpus → 200ms quota per 100ms period = 2 cores
        assert_eq!(cpu_max(2000), "200000 100000");
    }

    #[test]
    fn cpu_max_clamps_to_minimum_quota() {
        // very small allocations must still produce a kernel-acceptable value
        assert_eq!(cpu_max(0), "1000 100000");
        assert_eq!(cpu_max(5), "1000 100000");
    }

    // ── read_image_init ───────────────────────────────────────────────────────

    #[test]
    fn read_image_init_reads_sidecar_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ubuntu.init"), "/usr/sbin/init\n").unwrap();
        assert_eq!(read_image_init(dir.path(), "ubuntu"), "/usr/sbin/init");
    }

    #[test]
    fn read_image_init_trims_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("custom.init"), "  /sbin/runit  \n").unwrap();
        assert_eq!(read_image_init(dir.path(), "custom"), "/sbin/runit");
    }

    #[test]
    fn read_image_init_defaults_to_sbin_init_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_image_init(dir.path(), "nonexistent"), "/sbin/init");
    }
}
