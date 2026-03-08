use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::anyhow;

// a host is considered alive if heartbeat arrived within 30 seconds
const HEARTBEAT_TIMEOUT_SECS: i64 = 30;

pub async fn pick_host(pool: &db::PgPool) -> anyhow::Result<db::HostRow> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let hosts = db::list_hosts(pool).await?;
    let alive: Vec<_> = hosts.into_iter()
        .filter(|h| now - h.last_seen_at <= HEARTBEAT_TIMEOUT_SECS)
        .collect();

    if alive.is_empty() {
        return Err(anyhow!("no healthy hosts available — is a host-agent running?"));
    }

    // pick host with most free memory (mem_total_mb minus currently running VMs' memory)
    // for now use a simple heuristic: fewest running VMs
    let mut best: Option<db::HostRow> = None;
    let mut best_count: usize = usize::MAX;

    for host in alive {
        let vms = db::get_vms_by_host(pool, &host.id).await.unwrap_or_default();
        let running = vms.iter().filter(|v| v.status == "running").count();
        if running < best_count {
            best_count = running;
            best = Some(host);
        }
    }

    best.ok_or_else(|| anyhow!("no host selected"))
}

pub fn next_free_slot(used_ips: &[String]) -> u32 {
    for n in 1..=65534u32 {
        if !used_ips.iter().any(|ip| ip == &format!("172.16.{n}.2")) {
            return n;
        }
    }
    panic!("IP pool exhausted");
}
