use std::{path::PathBuf, time::Duration};

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
        resource::{ResourceType, system::ResourceSystem},
    },
};
use networking::{NetworkManager, iptables};

fn firecracker_path() -> PathBuf {
    std::env::var("FIRECRACKER_BIN")
        .unwrap_or_else(|_| "/usr/local/bin/firecracker".into())
        .into()
}

fn jailer_path() -> PathBuf {
    std::env::var("JAILER_BIN")
        .unwrap_or_else(|_| "/usr/local/bin/jailer".into())
        .into()
}

fn snapshot_editor_path() -> PathBuf {
    std::env::var("SNAPSHOT_EDITOR_BIN")
        .unwrap_or_else(|_| "/usr/local/bin/snapshot-editor".into())
        .into()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let kernel_path: PathBuf = std::env::var("KERNEL_PATH")
        .expect("KERNEL_PATH must be set")
        .into();
    let rootfs_path: PathBuf = std::env::var("ROOTFS_PATH")
        .expect("ROOTFS_PATH must be set")
        .into();
    let external_iface = std::env::var("EXTERNAL_IFACE").unwrap_or_else(|_| "eth0".into());

    // host networking setup
    let net = NetworkManager::new();
    iptables::enable_ip_forwarding()?;
    iptables::setup(&external_iface)?;


    let vm_id = common::VmId::new("spike-0");
    let tap = net.allocate_tap(&vm_id, 0)?;
    println!("TAP device: {} host={} guest={}", tap.name, tap.host_ip, tap.guest_ip);

    let boot_args = format!(
        "console=ttyS0 reboot=k panic=1 pci=off {}",
        networking::ip::kernel_boot_args(0)
    );

    let installation = VmmInstallation::new(firecracker_path(), jailer_path(), snapshot_editor_path());
    let socket_path = PathBuf::from(format!("/tmp/fc-{}.sock", vm_id.as_str()));
    let vmm_args = VmmArguments::new(VmmApiSocket::Enabled(socket_path));
    let executor = UnrestrictedVmmExecutor::new(vmm_args);

    let spawner = DirectProcessSpawner;
    let runtime = TokioRuntime;
    let resource_system = ResourceSystem::new(spawner, runtime, VmmOwnershipModel::Unconfined);

    let kernel_res = resource_system.create_resource(kernel_path, ResourceType::KernelImage)?;
    let rootfs_res = resource_system.create_resource(rootfs_path, ResourceType::RootFilesystem)?;

    let config = VmConfiguration::New {
        init_method: InitMethod::ViaApiCalls,
        data: VmConfigurationData {
            boot_source: BootSource {
                kernel_image: kernel_res,
                boot_args: Some(boot_args),
                initrd: None,
            },
            drives: vec![Drive {
                drive_id: "rootfs".into(),
                is_root_device: true,
                is_read_only: Some(false),
                block: Some(rootfs_res),
                cache_type: None,
                partuuid: None,
                rate_limiter: None,
                io_engine: None,
                socket: None,
            }],
            pmem_devices: vec![],
            machine_configuration: MachineConfiguration {
                vcpu_count: 2,
                mem_size_mib: 512,
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

    let mut vm = Vm::prepare(executor, resource_system, installation, config).await?;
    println!("VM prepared, starting...");

    vm.start(Duration::from_secs(5)).await?;
    println!("VM running. Guest IP: {}", tap.guest_ip);
    println!("Press Enter to stop...");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();

    vm.shutdown([
        VmShutdownAction { method: VmShutdownMethod::CtrlAltDel, timeout: Some(Duration::from_secs(8)) },
        VmShutdownAction { method: VmShutdownMethod::Kill, timeout: Some(Duration::from_secs(3)) },
    ])
    .await?;

    vm.cleanup().await?;

    net.release_tap(&vm_id)?;
    println!("Done.");

    Ok(())
}
