use std::collections::HashMap;

use router_sync::CaddyClient;
use tracing::warn;

/// Resolves the correct `CaddyClient` for a host based on its `region` label.
///
/// Configure per-region caddies via `CADDY_REGION_URLS=us-east=http://...,eu-west=http://...`.
/// Hosts without a `region` label (or an unrecognised region) fall back to the default client
/// configured by `CADDY_URL`.
#[derive(Clone)]
pub struct CaddyRouter {
    region_clients: HashMap<String, CaddyClient>,
    default: CaddyClient,
}

impl CaddyRouter {
    pub fn new(default: CaddyClient, region_clients: HashMap<String, CaddyClient>) -> Self {
        Self {
            region_clients,
            default,
        }
    }

    pub fn for_host(&self, host: &db::HostRow) -> CaddyClient {
        let region = host
            .labels
            .as_object()
            .and_then(|m| m.get("region"))
            .and_then(|v| v.as_str());
        self.for_region(region)
    }

    pub fn for_region(&self, region: Option<&str>) -> CaddyClient {
        region
            .and_then(|r| self.region_clients.get(r))
            .unwrap_or(&self.default)
            .clone()
    }

    /// All distinct `(region, client)` pairs: `(None, default)` first, then named regions.
    pub fn all_regions(&self) -> impl Iterator<Item = (Option<&str>, &CaddyClient)> {
        std::iter::once((None, &self.default)).chain(
            self.region_clients
                .iter()
                .map(|(r, c)| (Some(r.as_str()), c)),
        )
    }

    /// All distinct clients. The default is included; region clients that share
    /// the default URL are deduplicated.
    fn all_clients(&self) -> impl Iterator<Item = &CaddyClient> {
        let default_url = self.default.base_url();
        std::iter::once(&self.default).chain(
            self.region_clients
                .values()
                .filter(move |c| c.base_url() != default_url),
        )
    }

    /// Broadcast a set-running-route to every Caddy instance. Errors are
    /// logged and swallowed so a down PoP never blocks VM operations.
    pub async fn broadcast_set_vm_route(&self, subdomain: &str, ip: &str, port: u16) {
        for client in self.all_clients() {
            if let Err(e) = client.set_vm_route(subdomain, ip, port).await {
                warn!(caddy = client.base_url(), "set_vm_route {subdomain}: {e}");
            }
        }
    }

    /// Broadcast a delete to every Caddy instance.
    pub async fn broadcast_delete_route(&self, subdomain: &str) {
        for client in self.all_clients() {
            if let Err(e) = client.delete_route(subdomain).await {
                warn!(caddy = client.base_url(), "delete_route {subdomain}: {e}");
            }
        }
    }

    /// Broadcast a stopped-page route to every Caddy instance.
    pub async fn broadcast_set_stopped_route(&self, subdomain: &str) {
        for client in self.all_clients() {
            if let Err(e) = client.set_stopped_route(subdomain).await {
                warn!(caddy = client.base_url(), "set_stopped_route {subdomain}: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    fn client(url: &str) -> CaddyClient {
        CaddyClient::new(url, PathBuf::from("/dev/null"))
    }

    fn router() -> CaddyRouter {
        let mut regions = HashMap::new();
        regions.insert("us-east".into(), client("http://caddy-us"));
        regions.insert("eu-west".into(), client("http://caddy-eu"));
        CaddyRouter::new(client("http://caddy-default"), regions)
    }

    fn host_with_labels(labels: serde_json::Value) -> db::HostRow {
        db::HostRow {
            id: "h1".into(),
            name: "h1".into(),
            address: "http://h1:4000".into(),
            vcpu_total: 4,
            vcpu_used: 0,
            mem_total_mb: 4096,
            mem_used_mb: 0,
            images_dir: "/images".into(),
            overlay_dir: "/overlay".into(),
            snapshot_dir: "/snapshots".into(),
            kernel_path: "/vmlinux".into(),
            last_seen_at: 0,
            status: "active".into(),
            labels,
            snapshot_addr: "http://h1:8080".into(),
        }
    }

    #[test]
    fn for_host_known_region_returns_regional_client() {
        let r = router();
        let h = host_with_labels(json!({"region": "us-east"}));
        assert_eq!(r.for_host(&h).base_url(), "http://caddy-us");
    }

    #[test]
    fn for_host_unknown_region_falls_back_to_default() {
        let r = router();
        let h = host_with_labels(json!({"region": "ap-southeast"}));
        assert_eq!(r.for_host(&h).base_url(), "http://caddy-default");
    }

    #[test]
    fn for_host_no_region_label_falls_back_to_default() {
        let r = router();
        let h = host_with_labels(json!({}));
        assert_eq!(r.for_host(&h).base_url(), "http://caddy-default");
    }

    #[test]
    fn all_regions_contains_default_and_named() {
        let r = router();
        let pairs: Vec<_> = r.all_regions().map(|(reg, c)| (reg, c.base_url().to_string())).collect();
        assert_eq!(pairs[0], (None, "http://caddy-default".into()));
        let named: std::collections::HashSet<_> = pairs[1..].iter().map(|(reg, _)| reg.unwrap()).collect();
        assert!(named.contains("us-east"));
        assert!(named.contains("eu-west"));
        assert_eq!(pairs.len(), 3);
    }
}
