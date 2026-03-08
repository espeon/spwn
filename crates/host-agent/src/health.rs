use std::{sync::Arc, time::Duration};

use tracing::{error, warn};

use crate::manager::{VmEvent, VmManager};

pub async fn run_health_checks(manager: Arc<VmManager>) -> ! {
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let vms = match db::get_vms_by_status(&manager.pool, "running").await {
            Ok(v) => v,
            Err(e) => { error!("health check db error: {e}"); continue; }
        };
        for vm in vms {
            // only check VMs owned by this agent
            if vm.host_id.as_deref() != Some(&manager.host_id) {
                continue;
            }
            let Some(pid) = vm.pid else { continue };
            if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                warn!("health check: vm {} process dead (pid={pid})", vm.id);
                db::set_vm_status(&manager.pool, &vm.id, "error").await.ok();
                db::log_event(&manager.pool, &vm.id, "health_check_failed", Some("process_dead")).await.ok();
                let _ = manager.events.send(VmEvent::Crashed { vm_id: vm.id.clone() });
            }
        }
    }
}
