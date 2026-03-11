use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::anyhow;

// A host is considered alive if a heartbeat arrived within 60 seconds.
const HEARTBEAT_TIMEOUT_SECS: i64 = 60;

/// Pick a host for a new VM given resource and placement constraints.
///
/// `placement_strategy`:
///   - `"best_fit"` — prefer the host with the least remaining free memory
///     (packs VMs tightly, leaving other hosts free).
///   - `"spread"` — prefer the host with the most remaining free memory
///     (spreads load evenly across hosts).
///
/// `required_labels` is an optional JSON object; all key-value pairs must be
/// present in the host's labels map.
pub async fn pick_host(
    pool: &db::PgPool,
    vcpus_needed: i64,
    mem_mb_needed: i32,
    placement_strategy: &str,
    required_labels: Option<&serde_json::Value>,
) -> anyhow::Result<db::HostRow> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let hosts = db::list_active_hosts(pool).await?;

    let mut candidates: Vec<db::HostRow> = hosts
        .into_iter()
        .filter(|h| {
            if now - h.last_seen_at > HEARTBEAT_TIMEOUT_SECS {
                return false;
            }
            let free_vcpus = h.vcpu_total - h.vcpu_used;
            let free_mem = h.mem_total_mb - h.mem_used_mb;
            if free_vcpus < vcpus_needed || free_mem < mem_mb_needed {
                return false;
            }
            if let Some(required) = required_labels {
                if let Some(req_map) = required.as_object() {
                    if let Some(host_map) = h.labels.as_object() {
                        for (k, v) in req_map {
                            if host_map.get(k) != Some(v) {
                                return false;
                            }
                        }
                    } else {
                        return false;
                    }
                }
            }
            true
        })
        .collect();

    if candidates.is_empty() {
        return Err(anyhow!(
            "no healthy host with sufficient capacity \
             (need {vcpus_needed} vCPUs, {mem_mb_needed} MB) — \
             is a host-agent running?"
        ));
    }

    // best_fit: sort ascending by free mem (pack tightly).
    // spread: sort descending by free mem (spread load).
    candidates.sort_by_key(|h| h.mem_total_mb - h.mem_used_mb);
    if placement_strategy == "spread" {
        candidates.reverse();
    }

    Ok(candidates.remove(0))
}

pub fn next_free_slot(used_ips: &[String]) -> u32 {
    for n in 1..=65534u32 {
        if !used_ips.iter().any(|ip| ip == &format!("172.16.{n}.2")) {
            return n;
        }
    }
    panic!("IP pool exhausted");
}
