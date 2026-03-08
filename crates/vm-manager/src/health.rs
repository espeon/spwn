use std::{sync::Arc, time::Duration};

use tracing::{error, warn};

use crate::manager::VmManager;

pub async fn run_health_checks(manager: Arc<VmManager>) -> ! {
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let vms = match db::get_vms_by_status(&manager.pool, "running").await {
            Ok(v) => v,
            Err(e) => { error!("health check db error: {e}"); continue; }
        };
        for vm in vms {
            let Some(pid) = vm.pid else { continue };
            if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                warn!("health check: vm {} process dead (pid={pid})", vm.id);
                db::set_vm_status(&manager.pool, &vm.id, "error").await.ok();
                db::log_event(&manager.pool, &vm.id, "health_check_failed", Some("process_dead")).await.ok();
                if let Err(e) = manager.caddy.set_stopped_route(&vm.subdomain).await {
                    error!("failed to update caddy route for dead vm {}: {e}", vm.id);
                }
            }
        }
    }
}
