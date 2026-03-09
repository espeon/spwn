use std::{sync::Arc, time::Duration};

use tracing::{error, info, warn};

use crate::manager::VmManager;

pub async fn run_reconciliation(manager: Arc<VmManager>) -> ! {
    loop {
        if let Err(e) = reconcile_once(&manager).await {
            error!("reconciliation error: {e}");
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

pub async fn reconcile_once(manager: &VmManager) -> anyhow::Result<()> {
    info!("running reconciliation");

    let db_vms = db::get_vms_by_host(&manager.pool, &manager.host_id).await?;

    for vm in db_vms
        .iter()
        .filter(|v| v.status == "starting" || v.status == "stopping")
    {
        let is_alive = vm.pid.map_or(false, |pid| pid_is_alive(pid as u32));
        let new_status = if is_alive { "running" } else { "error" };
        warn!(
            "vm {} stuck in '{}', resetting to '{new_status}'",
            vm.id, vm.status
        );
        db::set_vm_status(&manager.pool, &vm.id, new_status)
            .await
            .ok();
        db::log_event(&manager.pool, &vm.id, "reconcile_stuck_reset", None)
            .await
            .ok();
    }

    for vm in db_vms.iter().filter(|v| v.status == "running") {
        if let Some(pid) = vm.pid {
            if !pid_is_alive(pid as u32) {
                warn!(
                    "vm {} has no running process (pid={pid}), marking error",
                    vm.id
                );
                db::set_vm_status(&manager.pool, &vm.id, "error").await.ok();
                db::log_event(&manager.pool, &vm.id, "reconcile_process_missing", None)
                    .await
                    .ok();
            }
        } else {
            // No PID recorded — try to recover one from the jailer cgroup.
            if let Some(pid) = read_pid_from_cgroup(&vm.id) {
                warn!(
                    "vm {} is running but has no pid; recovered pid={pid} from cgroup",
                    vm.id
                );
                db::set_vm_pid(&manager.pool, &vm.id, pid).await.ok();
            } else {
                warn!(
                    "vm {} is running but has no pid and no cgroup entry, marking error",
                    vm.id
                );
                db::set_vm_status(&manager.pool, &vm.id, "error").await.ok();
            }
        }
    }

    if let Ok(tap_names) = manager.networking.list_tap_devices() {
        let tracked_taps: std::collections::HashSet<_> = db_vms
            .iter()
            .filter_map(|v| v.tap_device.as_deref())
            .collect();

        for tap in &tap_names {
            if !tracked_taps.contains(tap.as_str()) {
                warn!("removing orphaned TAP device: {tap}");
                if let Some(slot) = tap
                    .strip_prefix("fc-tap-")
                    .and_then(|s| s.parse::<u32>().ok())
                {
                    manager.networking.release_tap(slot).ok();
                }
            }
        }
    }

    info!("reconciliation complete");
    Ok(())
}

// Check liveness via /proc/<pid>/status — works regardless of whether the
// process is jailed. The jailer's new PID namespace means we see the outer
// (host) PID in the pid file, so this is correct.
fn pid_is_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

// Read the first PID from the jailer-managed cgroup for a given VM.
// Path follows the default jailer cgroup layout: /sys/fs/cgroup/firecracker/<vm_id>/cgroup.procs
fn read_pid_from_cgroup(vm_id: &str) -> Option<i64> {
    let cgroup_procs = format!("/sys/fs/cgroup/firecracker/{vm_id}/cgroup.procs");
    let contents = std::fs::read_to_string(&cgroup_procs).ok()?;
    contents.lines().next()?.trim().parse::<i64>().ok()
}
