# phase 2: routing

**goal:** subdomain → VM IP routing through caddy. `curl https://<vmid>.yourdomain.com` hits a process inside the VM. stopped VM shows a fallback page.

**done when:**
- wildcard TLS cert issued via DNS-01 (use staging ACME for dev)
- running VM: curl subdomain → response from process inside VM
- stopped VM: curl subdomain → static "VM is stopped" HTML page
- websockets and SSE work through the proxy (verify with a test server inside VM)

---

## what to build

### `crates/router-sync/src/lib.rs`

caddy admin API client. caddy listens on `127.0.0.1:2019` (never `0.0.0.0` — see security note in plan-v2.md).

```rust
pub struct CaddyClient {
    base_url: String,  // "http://127.0.0.1:2019"
    client: reqwest::Client,
}

impl CaddyClient {
    pub fn new(base_url: &str) -> Self
    pub async fn set_vm_route(&self, subdomain: &str, vm_ip: &str, port: u16) -> Result<()>
    pub async fn set_stopped_route(&self, subdomain: &str) -> Result<()>
    pub async fn rebuild_all_routes(&self, routes: &[RouteEntry]) -> Result<()>
    pub async fn health(&self) -> Result<()>   // GET /config/ → 200
}

pub struct RouteEntry {
    pub subdomain: String,
    pub target: RouteTarget,
}

pub enum RouteTarget {
    Vm { ip: String, port: u16 },
    Stopped,
}
```

**caddy API shape** for a VM route:
```json
{
  "match": [{"host": ["<subdomain>.yourdomain.com"]}],
  "handle": [{
    "handler": "reverse_proxy",
    "upstreams": [{"dial": "172.16.N.2:8080"}],
    "flush_interval": -1,
    "transport": {
      "protocol": "http",
      "read_timeout": "300s",
      "write_timeout": "300s"
    }
  }]
}
```

**stopped route** replaces the `upstreams` target with the fallback static file server address (a small axum handler or a static file on disk served by caddy's `file_server`).

**important:** routes are identified by subdomain in the match condition. `rebuild_all_routes` does a full replace of the routes array — simpler than patching individual entries and safe because routes are rebuilt from DB on every startup anyway.

use `PATCH /config/apps/http/servers/main/routes` to replace the routes array atomically.

### static "VM is stopped" fallback

two options (pick one):
1. **caddy file_server** — write a static `stopped.html` to disk, have caddy serve it for stopped routes. zero runtime dependency.
2. **axum handler** — add a `/stopped` route to the API server that returns the HTML. stopped routes proxy to `127.0.0.1:3000/stopped?subdomain=<x>`.

option 1 is simpler for phase 2. option 2 is better long-term (can show VM status dynamically). start with option 1.

### caddy setup

```bash
# install xcaddy
go install github.com/caddyserver/xcaddy/cmd/xcaddy@latest

# build caddy with your DNS provider plugin (example: cloudflare)
xcaddy build --with github.com/caddy-dns/cloudflare \
  --output /usr/local/bin/caddy

# base Caddyfile (config/Caddyfile):
{
    admin localhost:2019
    email you@yourdomain.com
}

*.yourdomain.com {
    tls {
        dns cloudflare {env.CF_API_TOKEN}
    }
    # routes are injected dynamically via admin API
    # this block just establishes TLS and the catch-all
    respond "no route" 404
}
```

for local dev, skip real TLS:
```
*.localvm.dev {
    tls internal
    # add localvm.dev and *.localvm.dev to /etc/hosts pointing to 127.0.0.1
}
```

---

## dependencies to add

```toml
# router-sync/Cargo.toml
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
thiserror = "2"
common = { path = "../common" }
```

---

## verification checklist

```bash
# start caddy
caddy run --config config/Caddyfile

# start a test HTTP server inside the VM (from phase 1 spike)
# inside guest:
python3 -m http.server 8080

# on host — set route via router-sync (write a quick test binary or use curl directly)
curl -X PATCH http://127.0.0.1:2019/config/apps/http/servers/main/routes \
  -H "Content-Type: application/json" \
  -d '[{"match":[{"host":["test.localvm.dev"]}],"handle":[{"handler":"reverse_proxy","upstreams":[{"dial":"172.16.0.2:8080"}],"flush_interval":-1}]}]'

# test routing
curl -H "Host: test.localvm.dev" http://127.0.0.1/  # should hit guest's python server

# test websocket (optional but good to verify)
# inside guest: install wscat or run a simple ws echo server
# on host: wscat -c ws://test.localvm.dev/ws

# set stopped route + verify fallback HTML
```

---

## what's NOT in scope for phase 2

- DB (phase 3)
- auth (phase 5)
- subdomain generation (phase 3, when VM create API exists)
- route rebuilding from DB (phase 3, reconciliation loop)
