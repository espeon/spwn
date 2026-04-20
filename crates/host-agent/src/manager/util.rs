use std::{path::PathBuf, time::Duration};

use anyhow::{Context, anyhow};
use fctools::vmm::{id::VmmId, installation::VmmInstallation};

pub fn read_fc_log_tail(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    if content.is_empty() {
        return None;
    }
    let trimmed = if content.len() > 2000 {
        content[content.len() - 2000..].trim_start()
    } else {
        content.trim()
    };
    Some(trimmed.to_string())
}

pub(crate) fn jail_root_path(
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

pub(crate) fn cpu_weight(millis: i64) -> u64 {
    (millis / 10).clamp(1, 10000) as u64
}

pub(crate) const CPU_PERIOD_US: i64 = 100_000;

pub(crate) fn cpu_max(millis: i64) -> String {
    let quota = (millis * CPU_PERIOD_US / 1000).max(1000);
    format!("{quota} {CPU_PERIOD_US}")
}

pub(crate) fn memory_max(memory_mb: i32) -> String {
    ((memory_mb as i64) * 1024 * 1024).max(0).to_string()
}

pub(crate) fn ip_to_slot(guest_ip: &str) -> anyhow::Result<u32> {
    let parts: Vec<&str> = guest_ip.split('.').collect();
    if parts.len() != 4 {
        return Err(anyhow!("invalid guest IP: {guest_ip}"));
    }
    parts[2].parse::<u32>().context("parse slot from IP")
}

pub(crate) fn make_jail_id(vm_id: &str) -> anyhow::Result<VmmId> {
    VmmId::new(vm_id).map_err(|e| anyhow!("invalid jail id for vm {vm_id}: {e}"))
}

pub(crate) async fn read_jailer_pid(
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
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    read_pid_from_cgroup(vm_id)
}

fn read_pid_from_cgroup(vm_id: &str) -> Option<i64> {
    let cgroup_procs = format!("/sys/fs/cgroup/firecracker/{vm_id}/cgroup.procs");
    let contents = std::fs::read_to_string(&cgroup_procs).ok()?;
    contents.lines().next()?.trim().parse::<i64>().ok()
}

pub(crate) fn kill_pid(pid: i32) {
    use nix::{sys::signal, unistd::Pid};
    let _ = signal::kill(Pid::from_raw(pid), signal::Signal::SIGKILL);
}

pub(crate) fn read_image_init(images_dir: &std::path::Path, name: &str) -> String {
    let sidecar = images_dir.join(format!("{name}.init"));
    std::fs::read_to_string(&sidecar)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "/sbin/init".into())
}

pub(crate) async fn persist_overlay_from_jail(
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

    #[tokio::test]
    async fn read_jailer_pid_reads_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let vm_id = "vm-pid-test-abcd";
        let inst = installation(dir.path().join("firecracker").to_str().unwrap());

        let jail_root = dir.path().join("firecracker").join(vm_id).join("root");
        std::fs::create_dir_all(&jail_root).unwrap();
        std::fs::write(jail_root.join("firecracker.pid"), "12345\n").unwrap();

        let pid = read_jailer_pid(vm_id, dir.path(), &inst).await;
        assert_eq!(pid, Some(12345));
    }

    #[tokio::test]
    async fn read_jailer_pid_trims_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let vm_id = "vm-pid-trim-abcd";
        let inst = installation(dir.path().join("firecracker").to_str().unwrap());

        let jail_root = dir.path().join("firecracker").join(vm_id).join("root");
        std::fs::create_dir_all(&jail_root).unwrap();
        std::fs::write(jail_root.join("firecracker.pid"), "  99\n  ").unwrap();

        let pid = read_jailer_pid(vm_id, dir.path(), &inst).await;
        assert_eq!(pid, Some(99));
    }

    #[tokio::test]
    async fn read_jailer_pid_returns_none_when_no_pid_file_and_no_cgroup() {
        let dir = tempfile::tempdir().unwrap();
        let vm_id = "vm-pid-missing-abcd";
        let inst = installation(dir.path().join("firecracker").to_str().unwrap());

        let pid = read_jailer_pid(vm_id, dir.path(), &inst).await;
        assert_eq!(pid, None);
    }

    #[tokio::test]
    async fn read_jailer_pid_ignores_non_numeric_content() {
        let dir = tempfile::tempdir().unwrap();
        let vm_id = "vm-pid-bogus-abcd";
        let inst = installation(dir.path().join("firecracker").to_str().unwrap());

        let jail_root = dir.path().join("firecracker").join(vm_id).join("root");
        std::fs::create_dir_all(&jail_root).unwrap();
        std::fs::write(jail_root.join("firecracker.pid"), "not-a-pid\n").unwrap();

        let pid = read_jailer_pid(vm_id, dir.path(), &inst).await;
        assert_eq!(pid, None);
    }

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
        assert_eq!(cpu_max(1000), "100000 100000");
    }

    #[test]
    fn cpu_max_half_core() {
        assert_eq!(cpu_max(500), "50000 100000");
    }

    #[test]
    fn cpu_max_two_cores() {
        assert_eq!(cpu_max(2000), "200000 100000");
    }

    #[test]
    fn cpu_max_clamps_to_minimum_quota() {
        assert_eq!(cpu_max(0), "1000 100000");
        assert_eq!(cpu_max(5), "1000 100000");
    }

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
