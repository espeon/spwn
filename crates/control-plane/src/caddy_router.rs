use std::collections::HashMap;

use router_sync::CaddyClient;

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

    pub fn default_client(&self) -> &CaddyClient {
        &self.default
    }

    /// All distinct `(region, client)` pairs: `(None, default)` first, then named regions.
    pub fn all_regions(&self) -> impl Iterator<Item = (Option<&str>, &CaddyClient)> {
        std::iter::once((None, &self.default))
            .chain(self.region_clients.iter().map(|(r, c)| (Some(r.as_str()), c)))
    }
}
