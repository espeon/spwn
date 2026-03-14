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
    select_from_hosts(hosts, vcpus_needed, mem_mb_needed, placement_strategy, required_labels, now)
}

/// Pure selection logic — extracted for unit-testability.
fn select_from_hosts(
    hosts: Vec<db::HostRow>,
    vcpus_needed: i64,
    mem_mb_needed: i32,
    placement_strategy: &str,
    required_labels: Option<&serde_json::Value>,
    now: i64,
) -> anyhow::Result<db::HostRow> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn host(id: &str, vcpu_total: i64, vcpu_used: i64, mem_total_mb: i32, mem_used_mb: i32, labels: serde_json::Value, last_seen_offset: i64) -> db::HostRow {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        db::HostRow {
            id: id.to_string(),
            name: id.to_string(),
            address: format!("http://{id}:4000"),
            vcpu_total,
            vcpu_used,
            mem_total_mb,
            mem_used_mb,
            images_dir: "/images".into(),
            overlay_dir: "/overlay".into(),
            snapshot_dir: "/snapshots".into(),
            kernel_path: "/vmlinux".into(),
            last_seen_at: now - last_seen_offset,
            status: "active".into(),
            labels,
            snapshot_addr: format!("http://{id}:8080"),
        }
    }

    fn now() -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
    }

    #[test]
    fn picks_host_with_capacity() {
        let hosts = vec![host("a", 4000, 0, 4096, 0, json!({}), 0)];
        let result = select_from_hosts(hosts, 1000, 512, "best_fit", None, now());
        assert_eq!(result.unwrap().id, "a");
    }

    #[test]
    fn rejects_all_if_none_have_capacity() {
        let hosts = vec![host("a", 1000, 1000, 512, 512, json!({}), 0)];
        assert!(select_from_hosts(hosts, 1000, 512, "best_fit", None, now()).is_err());
    }

    #[test]
    fn rejects_stale_hosts() {
        let hosts = vec![
            host("stale", 4000, 0, 4096, 0, json!({}), HEARTBEAT_TIMEOUT_SECS + 1),
            host("fresh", 4000, 0, 4096, 0, json!({}), 0),
        ];
        let result = select_from_hosts(hosts, 1000, 512, "best_fit", None, now()).unwrap();
        assert_eq!(result.id, "fresh");
    }

    #[test]
    fn best_fit_picks_least_free_mem() {
        let hosts = vec![
            host("a", 8000, 0, 8192, 0, json!({}), 0),   // 8192 MB free
            host("b", 8000, 0, 4096, 0, json!({}), 0),   // 4096 MB free
        ];
        let result = select_from_hosts(hosts, 1000, 512, "best_fit", None, now()).unwrap();
        assert_eq!(result.id, "b");
    }

    #[test]
    fn spread_picks_most_free_mem() {
        let hosts = vec![
            host("a", 8000, 0, 8192, 0, json!({}), 0),
            host("b", 8000, 0, 4096, 0, json!({}), 0),
        ];
        let result = select_from_hosts(hosts, 1000, 512, "spread", None, now()).unwrap();
        assert_eq!(result.id, "a");
    }

    #[test]
    fn label_filter_excludes_mismatched_hosts() {
        let hosts = vec![
            host("us", 4000, 0, 4096, 0, json!({"region": "us-east"}), 0),
            host("eu", 4000, 0, 4096, 0, json!({"region": "eu-west"}), 0),
        ];
        let required = json!({"region": "us-east"});
        let result = select_from_hosts(hosts, 1000, 512, "best_fit", Some(&required), now()).unwrap();
        assert_eq!(result.id, "us");
    }

    #[test]
    fn label_filter_no_match_returns_err() {
        let hosts = vec![host("a", 4000, 0, 4096, 0, json!({"region": "us-east"}), 0)];
        let required = json!({"region": "ap-southeast"});
        assert!(select_from_hosts(hosts, 1000, 512, "best_fit", Some(&required), now()).is_err());
    }

    #[test]
    fn next_free_slot_skips_used() {
        let used = vec!["172.16.1.2".to_string(), "172.16.2.2".to_string()];
        assert_eq!(next_free_slot(&used), 3);
    }

    #[test]
    fn next_free_slot_first_when_empty() {
        assert_eq!(next_free_slot(&[]), 1);
    }
}

pub fn next_free_slot(used_ips: &[String]) -> u32 {
    for n in 1..=65534u32 {
        if !used_ips.iter().any(|ip| ip == &format!("172.16.{n}.2")) {
            return n;
        }
    }
    panic!("IP pool exhausted");
}
