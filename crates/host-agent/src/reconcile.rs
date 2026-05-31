use std::{path::PathBuf, sync::Arc, time::Duration};

use tracing::{error, info, warn};

use crate::{manager::VmManager, overlay};

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

    // Update real disk usage for every VM that has an overlay on this host.
    for vm in &db_vms {
        let Some(overlay_path) = vm.overlay_path.as_deref() else {
            continue;
        };
        let path = PathBuf::from(overlay_path);
        if !path.exists() {
            continue;
        }
        let usage = overlay::measure_overlay_usage_mb(&path);
        db::update_disk_usage_mb(&manager.pool, &vm.id, usage)
            .await
            .ok();
    }

    for vm in db_vms
        .iter()
        .filter(|v| v.status == "starting" || v.status == "stopping")
    {
        let is_alive = vm.pid.is_some_and(|pid| pid_is_alive(pid as u32));
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

    // Rebuild monitored_vm_ids so the metrics collector can find running VMs
    // even after a host-agent restart (the in-process `running` HashMap is not
    // persisted, but cgroup PIDs survive).
    let alive_ids: std::collections::HashSet<String> = db_vms
        .iter()
        .filter(|v| v.status == "running")
        .filter_map(|v| v.pid.map(|pid| (v.id.clone(), pid)))
        .filter(|(_, pid)| pid_is_alive(*pid as u32))
        .map(|(id, _)| id)
        .collect();
    *manager.monitored_vm_ids.lock().await = alive_ids;

    // Recover VMs the DB thinks are stopped but actually have a live process.
    // This happens when the event stream drops during a VM start — the "started"
    // event is lost, so the control-plane never updates the status or Caddy route.
    for vm in db_vms.iter().filter(|v| v.status == "stopped" || v.status == "error") {
        if let Some(pid) = read_pid_from_cgroup(&vm.id) {
            if pid_is_alive(pid as u32) {
                warn!(
                    "vm {} is '{}' in DB but has live process (pid={pid}), recovering",
                    vm.id, vm.status
                );
                db::set_vm_pid(&manager.pool, &vm.id, pid).await.ok();
                let _ = manager.events.send(crate::manager::VmEvent::Started {
                    vm_id: vm.id.clone(),
                });
                db::log_event(&manager.pool, &vm.id, "reconcile_running_recovery", None)
                    .await
                    .ok();
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

    // Claim any VMs whose overlay lives on this host but have no host assignment.
    // This repairs VMs created before the set_vm_host fix was deployed.
    let unassigned = db::get_vms_without_host(&manager.pool).await?;
    for vm in unassigned {
        let Some(overlay_path) = vm.overlay_path.as_deref() else {
            continue;
        };
        if PathBuf::from(overlay_path).exists() {
            info!("claiming unassigned vm {} (overlay found locally)", vm.id);
            db::set_vm_host(&manager.pool, &vm.id, &manager.host_id)
                .await
                .ok();
            db::log_event(&manager.pool, &vm.id, "reconcile_host_claimed", None)
                .await
                .ok();
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
    parse_first_pid(&contents)
}

fn parse_first_pid(contents: &str) -> Option<i64> {
    contents.lines().next()?.trim().parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::{parse_first_pid, pid_is_alive};

    // ── pid_is_alive ──────────────────────────────────────────────────────────

    #[test]
    fn pid_is_alive_returns_true_for_current_process() {
        let pid = std::process::id();
        assert!(pid_is_alive(pid), "current process should be alive");
    }

    #[test]
    fn pid_is_alive_returns_false_for_pid_1_if_not_init() {
        // PID 1 always exists on Linux (init/systemd), so this is a sanity
        // check that the function can return true for a real pid.
        assert!(pid_is_alive(1), "pid 1 should always exist on Linux");
    }

    #[test]
    fn pid_is_alive_returns_false_for_implausible_pid() {
        // PID 0 is never a real process.
        assert!(!pid_is_alive(0));
    }

    // ── parse_first_pid ───────────────────────────────────────────────────────

    #[test]
    fn parse_first_pid_reads_single_pid() {
        assert_eq!(parse_first_pid("12345\n"), Some(12345));
    }

    #[test]
    fn parse_first_pid_reads_first_of_multiple_pids() {
        assert_eq!(parse_first_pid("12345\n67890\n"), Some(12345));
    }

    #[test]
    fn parse_first_pid_trims_whitespace() {
        assert_eq!(parse_first_pid("  99  \n"), Some(99));
    }

    #[test]
    fn parse_first_pid_returns_none_for_empty_input() {
        assert_eq!(parse_first_pid(""), None);
    }

    #[test]
    fn parse_first_pid_returns_none_for_whitespace_only() {
        assert_eq!(parse_first_pid("   \n"), None);
    }

    #[test]
    fn parse_first_pid_returns_none_for_non_numeric() {
        assert_eq!(parse_first_pid("not-a-pid\n"), None);
    }

    #[test]
    fn parse_first_pid_parses_negative_as_valid_i64() {
        // parse_first_pid is a straight i64 parse — negative values won't
        // appear in a real cgroup.procs file but we don't need to reject them
        assert_eq!(parse_first_pid("-1\n"), Some(-1));
    }
}
