use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;
use tracing::{trace, warn};

use crate::manager::util::ip_to_slot;

const RING_BUFFER_SIZE: usize = 300; // 25 min at 5 s intervals
const CGROUP_BASE: &str = "/sys/fs/cgroup/firecracker";
const COLLECTION_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct MetricsSample {
    pub vm_id: String,
    pub timestamp: i64,
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub memory_limit_bytes: u64,
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
}

struct VmCollectionState {
    samples: VecDeque<MetricsSample>,
    last_cpu_usec: Option<u64>,
    last_cpu_instant: Option<Instant>,
}

impl VmCollectionState {
    fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(RING_BUFFER_SIZE + 1),
            last_cpu_usec: None,
            last_cpu_instant: None,
        }
    }

    fn push(&mut self, sample: MetricsSample) {
        if self.samples.len() >= RING_BUFFER_SIZE {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }
}

pub struct MetricsStore {
    inner: RwLock<HashMap<String, VmCollectionState>>,
}

impl MetricsStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(HashMap::new()),
        })
    }

    /// Returns recent samples for one VM.
    pub async fn get_samples(&self, vm_id: &str, limit: usize) -> Vec<MetricsSample> {
        let inner = self.inner.read().await;
        let Some(state) = inner.get(vm_id) else {
            return vec![];
        };
        let all: Vec<_> = state.samples.iter().cloned().collect();
        if limit == 0 || limit >= all.len() {
            all
        } else {
            all[all.len() - limit..].to_vec()
        }
    }

    /// Returns the latest sample for every VM that has data.
    pub async fn get_latest_all(&self) -> Vec<MetricsSample> {
        let inner = self.inner.read().await;
        inner
            .values()
            .filter_map(|s| s.samples.back().cloned())
            .collect()
    }

    /// Remove state for VMs that are no longer running.
    async fn evict(&self, running_ids: &[String]) {
        let mut inner = self.inner.write().await;
        inner.retain(|id, _| running_ids.contains(id));
    }
}

// ── collection ────────────────────────────────────────────────────────────────

pub async fn run_collector(
    store: Arc<MetricsStore>,
    manager: Arc<crate::manager::VmManager>,
) {
    let mut interval = tokio::time::interval(COLLECTION_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        interval.tick().await;

        // Collect running VM IDs under a brief lock.
        let running_ids: Vec<String> = {
            let running = manager.running.lock().await;
            running.keys().cloned().collect()
        };

        if running_ids.is_empty() {
            store.evict(&running_ids).await;
            continue;
        }

        // Fetch IPs from DB (needed for TAP device slot lookup).
        let vm_ips: HashMap<String, String> = match db::get_vms_by_host(&manager.pool, &manager.host_id).await {
            Ok(rows) => rows
                .into_iter()
                .filter(|r| running_ids.contains(&r.id))
                .map(|r| (r.id, r.ip_address))
                .collect(),
            Err(e) => {
                warn!("metrics: failed to fetch VM IPs: {e}");
                HashMap::new()
            }
        };

        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let now_instant = Instant::now();

        let mut inner = store.inner.write().await;

        for vm_id in &running_ids {
            let state = inner.entry(vm_id.clone()).or_insert_with(VmCollectionState::new);
            let ip = vm_ips.get(vm_id).map(String::as_str);

            let sample = collect_one(vm_id, ip, now_unix, now_instant, state);
            state.push(sample);
        }

        // Drop any VMs that stopped between ticks.
        inner.retain(|id, _| running_ids.contains(id));
    }
}

fn collect_one(
    vm_id: &str,
    ip: Option<&str>,
    timestamp: i64,
    now: Instant,
    state: &mut VmCollectionState,
) -> MetricsSample {
    let cgroup = format!("{CGROUP_BASE}/{vm_id}");

    let (memory_bytes, memory_limit_bytes) = read_memory(&cgroup);
    let (disk_read_bytes, disk_write_bytes) = read_io(&cgroup);
    let cpu_percent = read_cpu_percent(&cgroup, now, state);
    let (net_rx_bytes, net_tx_bytes) = ip
        .and_then(|ip| ip_to_slot(ip).ok())
        .map(|slot| read_net(slot))
        .unwrap_or((0, 0));

    MetricsSample {
        vm_id: vm_id.to_owned(),
        timestamp,
        cpu_percent,
        memory_bytes,
        memory_limit_bytes,
        disk_read_bytes,
        disk_write_bytes,
        net_rx_bytes,
        net_tx_bytes,
    }
}

// ── cgroup readers ────────────────────────────────────────────────────────────

fn read_memory(cgroup: &str) -> (u64, u64) {
    let current = std::fs::read_to_string(format!("{cgroup}/memory.current"))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);

    let limit = std::fs::read_to_string(format!("{cgroup}/memory.max"))
        .ok()
        .and_then(|s| {
            let t = s.trim();
            if t == "max" {
                None
            } else {
                t.parse::<u64>().ok()
            }
        })
        .unwrap_or(0);

    (current, limit)
}

/// Returns cumulative disk read/write bytes from io.stat.
/// Format: `<major>:<minor> rbytes=N wbytes=N rios=N wios=N dbytes=N dios=N`
fn read_io(cgroup: &str) -> (u64, u64) {
    let content = match std::fs::read_to_string(format!("{cgroup}/io.stat")) {
        Ok(s) => s,
        Err(_) => return (0, 0),
    };

    let mut rbytes = 0u64;
    let mut wbytes = 0u64;

    for line in content.lines() {
        for field in line.split_whitespace().skip(1) {
            if let Some(v) = field.strip_prefix("rbytes=") {
                rbytes += v.parse::<u64>().unwrap_or(0);
            } else if let Some(v) = field.strip_prefix("wbytes=") {
                wbytes += v.parse::<u64>().unwrap_or(0);
            }
        }
    }

    (rbytes, wbytes)
}

/// Returns CPU% over the last collection interval using usage_usec delta.
fn read_cpu_percent(
    cgroup: &str,
    now: Instant,
    state: &mut VmCollectionState,
) -> f64 {
    let usage_usec = match read_cpu_usage_usec(cgroup) {
        Some(v) => v,
        None => return 0.0,
    };

    let result = match (state.last_cpu_usec, state.last_cpu_instant) {
        (Some(prev_usec), Some(prev_instant)) => {
            let delta_usec = usage_usec.saturating_sub(prev_usec) as f64;
            let elapsed_usec = now.duration_since(prev_instant).as_micros() as f64;
            if elapsed_usec > 0.0 {
                (delta_usec / elapsed_usec * 100.0).min(100.0 * num_cpus())
            } else {
                0.0
            }
        }
        _ => 0.0,
    };

    state.last_cpu_usec = Some(usage_usec);
    state.last_cpu_instant = Some(now);
    result
}

fn read_cpu_usage_usec(cgroup: &str) -> Option<u64> {
    let content = std::fs::read_to_string(format!("{cgroup}/cpu.stat")).ok()?;
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("usage_usec ") {
            return v.trim().parse::<u64>().ok();
        }
    }
    None
}

fn num_cpus() -> f64 {
    std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0)
}

// ── network ───────────────────────────────────────────────────────────────────

/// Reads cumulative rx/tx bytes for fc-tap-{slot} from /proc/net/dev.
fn read_net(slot: u32) -> (u64, u64) {
    let target = format!("fc-tap-{slot}:");
    let content = match std::fs::read_to_string("/proc/net/dev") {
        Ok(s) => s,
        Err(_) => return (0, 0),
    };

    for line in content.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with(&target) {
            continue;
        }
        // Format: "fc-tap-N: rx_bytes rx_packets ... tx_bytes ..."
        // Fields after the colon: rx_bytes(0) rx_packets(1) rx_errs(2) rx_drop(3)
        //   rx_fifo(4) rx_frame(5) rx_compressed(6) rx_multicast(7)
        //   tx_bytes(8) tx_packets(9) ...
        let after_colon = trimmed
            .splitn(2, ':')
            .nth(1)
            .unwrap_or("")
            .trim();
        let fields: Vec<u64> = after_colon
            .split_whitespace()
            .filter_map(|f| f.parse().ok())
            .collect();

        let rx = fields.first().copied().unwrap_or(0);
        let tx = fields.get(8).copied().unwrap_or(0);
        return (rx, tx);
    }

    trace!("metrics: fc-tap-{slot} not found in /proc/net/dev");
    (0, 0)
}
