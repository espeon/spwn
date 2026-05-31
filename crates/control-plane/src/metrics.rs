use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_proto::agent::host_agent_client::HostAgentClient;
use agent_proto::agent::{GetVmMetricsRequest, VmMetricsSample};
use axum::{
    Extension,
    extract::{MatchedPath, Path, Request},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::IntoResponse,
};
use prometheus::{CounterVec, Encoder, GaugeVec, HistogramVec, TextEncoder, register_counter_vec, register_gauge_vec, register_histogram_vec};
use serde::Serialize;
use tokio::sync::RwLock;
use tracing::warn;

const RING_BUFFER_SIZE: usize = 360; // 1 hour at 10 s intervals
const POLL_INTERVAL: Duration = Duration::from_secs(10);

// ── sample type exposed to the API layer ──────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct MetricSample {
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

impl From<VmMetricsSample> for MetricSample {
    fn from(s: VmMetricsSample) -> Self {
        Self {
            vm_id: s.vm_id,
            timestamp: s.timestamp,
            cpu_percent: s.cpu_percent,
            memory_bytes: s.memory_bytes,
            memory_limit_bytes: s.memory_limit_bytes,
            disk_read_bytes: s.disk_read_bytes,
            disk_write_bytes: s.disk_write_bytes,
            net_rx_bytes: s.net_rx_bytes,
            net_tx_bytes: s.net_tx_bytes,
        }
    }
}

// ── cache ─────────────────────────────────────────────────────────────────────

pub struct MetricsCache {
    samples: RwLock<HashMap<String, VecDeque<MetricSample>>>,
    cpu_gauge: GaugeVec,
    memory_gauge: GaugeVec,
    memory_limit_gauge: GaugeVec,
    net_rx_gauge: GaugeVec,
    net_tx_gauge: GaugeVec,
    vms_total_gauge: GaugeVec,
    // request metrics — registered once, used by the middleware
    pub request_counter: CounterVec,
    pub request_duration: HistogramVec,
}

impl MetricsCache {
    pub fn new() -> Arc<Self> {
        let labels = &["vm_id", "vm_name"];
        Arc::new(Self {
            samples: RwLock::new(HashMap::new()),
            cpu_gauge: register_gauge_vec!(
                "spwn_vm_cpu_percent",
                "VM CPU usage as a percentage of one core",
                labels
            )
            .expect("register spwn_vm_cpu_percent"),
            memory_gauge: register_gauge_vec!(
                "spwn_vm_memory_bytes",
                "VM memory usage in bytes",
                labels
            )
            .expect("register spwn_vm_memory_bytes"),
            memory_limit_gauge: register_gauge_vec!(
                "spwn_vm_memory_limit_bytes",
                "VM memory limit in bytes",
                labels
            )
            .expect("register spwn_vm_memory_limit_bytes"),
            net_rx_gauge: register_gauge_vec!(
                "spwn_vm_net_rx_bytes_total",
                "VM cumulative network receive bytes",
                labels
            )
            .expect("register spwn_vm_net_rx_bytes_total"),
            net_tx_gauge: register_gauge_vec!(
                "spwn_vm_net_tx_bytes_total",
                "VM cumulative network transmit bytes",
                labels
            )
            .expect("register spwn_vm_net_tx_bytes_total"),
            vms_total_gauge: register_gauge_vec!(
                "spwn_vms_total",
                "Number of VMs by status",
                &["status"]
            )
            .expect("register spwn_vms_total"),
            request_counter: register_counter_vec!(
                "spwn_api_requests_total",
                "Total HTTP requests by method, path, and status",
                &["method", "path", "status"]
            )
            .expect("register spwn_api_requests_total"),
            request_duration: register_histogram_vec!(
                "spwn_api_request_duration_seconds",
                "HTTP request duration in seconds",
                &["method", "path", "status"],
                vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0]
            )
            .expect("register spwn_api_request_duration_seconds"),
        })
    }

    pub async fn get_samples(&self, vm_id: &str, limit: usize) -> Vec<MetricSample> {
        let samples = self.samples.read().await;
        let Some(ring) = samples.get(vm_id) else {
            return vec![];
        };
        let all: Vec<_> = ring.iter().cloned().collect();
        if limit == 0 || limit >= all.len() {
            all
        } else {
            all[all.len() - limit..].to_vec()
        }
    }

    async fn ingest(&self, sample: MetricSample, vm_name: &str) {
        let vm_id = sample.vm_id.clone();
        let labels = [vm_id.as_str(), vm_name];

        self.cpu_gauge.with_label_values(&labels).set(sample.cpu_percent);
        self.memory_gauge.with_label_values(&labels).set(sample.memory_bytes as f64);
        self.memory_limit_gauge.with_label_values(&labels).set(sample.memory_limit_bytes as f64);
        self.net_rx_gauge.with_label_values(&labels).set(sample.net_rx_bytes as f64);
        self.net_tx_gauge.with_label_values(&labels).set(sample.net_tx_bytes as f64);

        let mut s = self.samples.write().await;
        let ring = s.entry(vm_id).or_insert_with(|| VecDeque::with_capacity(RING_BUFFER_SIZE + 1));
        if ring.len() >= RING_BUFFER_SIZE {
            ring.pop_front();
        }
        ring.push_back(sample);
    }

    fn update_vm_counts(&self, counts: HashMap<String, usize>) {
        for (status, count) in counts {
            self.vms_total_gauge
                .with_label_values(&[&status])
                .set(count as f64);
        }
    }

    async fn evict_stale(&self, active_vm_ids: &[String]) {
        let mut s = self.samples.write().await;
        s.retain(|id, _| active_vm_ids.contains(id));
    }
}

// ── poller ────────────────────────────────────────────────────────────────────

pub async fn run_poller(cache: Arc<MetricsCache>, pool: db::PgPool, tls: Option<crate::tls::GrpcTls>) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        interval.tick().await;

        // Update VM status counts from DB.
        if let Ok(vms) = db::get_all_vms(&pool).await {
            let mut counts: HashMap<String, usize> = HashMap::new();
            for vm in &vms {
                *counts.entry(vm.status.clone()).or_default() += 1;
            }
            cache.update_vm_counts(counts);
        }

        // Collect per-VM metrics from each active host.
        let hosts = match db::list_active_hosts(&pool).await {
            Ok(h) => h,
            Err(e) => {
                warn!("metrics poller: list hosts: {e}");
                continue;
            }
        };

        // Build a map of vm_id → vm_name for label population.
        let vm_names: HashMap<String, String> = match db::get_all_vms(&pool).await {
            Ok(vms) => vms.into_iter().map(|v| (v.id, v.name)).collect(),
            Err(_) => HashMap::new(),
        };

        let mut active_vm_ids = Vec::new();

        for host in hosts {
            let channel = match crate::tls::agent_channel(&host.address, tls.as_ref()).await {
                Ok(c) => c,
                Err(e) => {
                    warn!("metrics poller: connect to host {}: {e}", host.id);
                    continue;
                }
            };

            let mut client = HostAgentClient::new(channel);
            let resp = match client
                .get_vm_metrics(GetVmMetricsRequest {
                    vm_id: String::new(), // all VMs
                    limit: 1,            // latest sample only
                })
                .await
            {
                Ok(r) => r.into_inner(),
                Err(e) => {
                    warn!("metrics poller: GetVmMetrics from host {}: {e}", host.id);
                    continue;
                }
            };

            for proto_sample in resp.samples {
                active_vm_ids.push(proto_sample.vm_id.clone());
                let name = vm_names
                    .get(&proto_sample.vm_id)
                    .map(String::as_str)
                    .unwrap_or("unknown");
                cache.ingest(MetricSample::from(proto_sample), name).await;
            }
        }

        cache.evict_stale(&active_vm_ids).await;
    }
}

// ── HTTP handlers ─────────────────────────────────────────────────────────────

/// GET /metrics — Prometheus/VictoriaMetrics scrape endpoint.
pub async fn prometheus_handler() -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let families = prometheus::gather();
    let mut buf = Vec::new();
    if let Err(e) = encoder.encode(&families, &mut buf) {
        return (StatusCode::INTERNAL_SERVER_ERROR, HeaderMap::new(), format!("encode error: {e}").into_bytes());
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        encoder.format_type().parse().expect("valid content type"),
    );
    (StatusCode::OK, headers, buf)
}

/// GET /api/vms/:id/metrics?limit=<n>
pub async fn vm_metrics_handler(
    Path(vm_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<MetricsQuery>,
    Extension(cache): Extension<Arc<MetricsCache>>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(60);
    let samples = cache.get_samples(&vm_id, limit).await;
    axum::Json(samples)
}

#[derive(serde::Deserialize)]
pub struct MetricsQuery {
    pub limit: Option<usize>,
}

// ── request metrics middleware ─────────────────────────────────────────────────

pub async fn track_requests(
    Extension(cache): Extension<Arc<MetricsCache>>,
    matched_path: Option<MatchedPath>,
    req: Request,
    next: Next,
) -> impl IntoResponse {
    let method = req.method().to_string();
    let path = matched_path
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());

    let start = Instant::now();
    let resp = next.run(req).await;
    let elapsed = start.elapsed().as_secs_f64();
    let status = resp.status().as_u16().to_string();

    let labels = [method.as_str(), path.as_str(), status.as_str()];
    cache.request_counter.with_label_values(&labels).inc();
    cache.request_duration.with_label_values(&labels).observe(elapsed);

    resp
}
