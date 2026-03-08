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

    let firecracker_pids = find_firecracker_pids();
    let db_vms = db::get_all_vms(&manager.pool).await?;

    // reset VMs stuck in transitional states (starting/stopping) from a previous crash
    for vm in db_vms.iter().filter(|v| v.status == "starting" || v.status == "stopping") {
        let is_alive = vm.pid.map_or(false, |pid| firecracker_pids.contains(&(pid as i32)));
        let new_status = if is_alive { "running" } else { "error" };
        warn!("vm {} stuck in '{}', resetting to '{new_status}'", vm.id, vm.status);
        db::set_vm_status(&manager.pool, &vm.id, new_status).await.ok();
        db::log_event(&manager.pool, &vm.id, "reconcile_stuck_reset", None).await.ok();
    }

    // mark DB-running VMs whose process is gone as error
    for vm in db_vms.iter().filter(|v| v.status == "running") {
        if let Some(pid) = vm.pid {
            if !firecracker_pids.contains(&(pid as i32)) {
                warn!("vm {} has no running process (pid={pid}), marking error", vm.id);
                db::set_vm_status(&manager.pool, &vm.id, "error").await.ok();
                db::log_event(&manager.pool, &vm.id, "reconcile_process_missing", None).await.ok();
            }
        } else {
            // running but no PID recorded — mark error
            warn!("vm {} is running but has no pid, marking error", vm.id);
            db::set_vm_status(&manager.pool, &vm.id, "error").await.ok();
        }
    }

    // clean up orphaned TAP devices
    if let Ok(tap_names) = manager.networking.list_tap_devices() {
        let tracked_taps: std::collections::HashSet<_> = db_vms.iter()
            .filter_map(|v| v.tap_device.as_deref())
            .collect();

        for tap in &tap_names {
            if !tracked_taps.contains(tap.as_str()) {
                warn!("removing orphaned TAP device: {tap}");
                // name format: fc-tap-{slot}
                if let Some(slot) = tap.strip_prefix("fc-tap-").and_then(|s| s.parse::<u32>().ok()) {
                    manager.networking.release_tap(slot).ok();
                }
            }
        }
    }

    // rebuild caddy routes from DB state
    rebuild_caddy_routes(manager).await;

    info!("reconciliation complete");
    Ok(())
}

async fn rebuild_caddy_routes(manager: &VmManager) {
    let vms = match db::get_all_vms(&manager.pool).await {
        Ok(v) => v,
        Err(e) => { error!("failed to load vms for route rebuild: {e}"); return; }
    };

    let routes: Vec<router_sync::RouteEntry> = vms.into_iter().map(|vm| {
        let target = if vm.status == "running" {
            router_sync::RouteTarget::Vm { ip: vm.ip_address.clone(), port: vm.exposed_port as u16 }
        } else {
            router_sync::RouteTarget::Stopped
        };
        router_sync::RouteEntry { subdomain: vm.subdomain, target }
    }).collect();

    if let Err(e) = manager.caddy.rebuild_all_routes(&routes).await {
        error!("failed to rebuild caddy routes: {e}");
    }
}

fn find_firecracker_pids() -> Vec<i32> {
    let Ok(proc) = std::fs::read_dir("/proc") else { return vec![] };
    let mut pids = Vec::new();
    for entry in proc.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let cmdline_path = format!("/proc/{name}/cmdline");
        if let Ok(cmdline) = std::fs::read_to_string(&cmdline_path) {
            if cmdline.contains("firecracker") {
                if let Ok(pid) = name.parse::<i32>() {
                    pids.push(pid);
                }
            }
        }
    }
    pids
}
