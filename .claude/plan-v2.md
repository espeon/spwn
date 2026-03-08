# ephemeral vm platform - build plan (v2)

## what we're building

a hobbyist vm platform: $24/mo gets you a fixed resource pool (8 vcores, 12gb ram), persistent microVMs via firecracker, and wildcard subdomain routing with auto-tls. target users are devs who want cheap sandboxes with snapshot/restore.

---

## tech stack

| layer | choice |
|---|---|
| vm runtime | firecracker + KVM (with jailer) |
| router | caddy (runtime API for dynamic config, built with xcaddy + DNS plugin) |
| backend api | rust / axum / tower |
| database | postgres via sqlx |
| auth | axum-login + tower-sessions + argon2 |
| billing | lemonsqueezy (webhooks) |
| frontend | react + tanstack router + tanstack query |
| hosting | hetzner AX52 or similar (16c/128gb) |

### auth rationale

better-auth is a JS/TS library, which would require either a sidecar Node process or reimplementing its session logic in Rust. the auth surface here is small (signup, login, session management) — `axum-login` with argon2 password hashing and `tower-sessions` integrates cleanly into the Rust stack without an extra runtime dependency.

---

## system architecture

```
                        internet
                           |
                        caddy
                    (TLS termination,
                  wildcard subdomain routing,
                  built via xcaddy w/ DNS plugin)
                           |
              ┌────────────┴────────────┐
              |                         |
         api-server              vm http traffic
         (axum, :3000)          (proxy to VM TAP IPs)
              |
         vm-manager
         (tokio, manages
          firecracker procs,
          reconciliation loop,
          health checks)
              |
      ┌───────┴───────┐
    FC VM 1        FC VM N
   (jailer +      (jailer +
    TAP device)    TAP device)
```

### firecracker networking (detailed)

each VM gets a TAP device on the host. networking uses static IP assignment (no DHCP) with a deterministic CIDR scheme:

```
network:     172.16.0.0/16
VM N host:   172.16.N.1/30   (assigned to TAP device on host)
VM N guest:  172.16.N.2/30   (configured via kernel boot args)
```

guest IP is set via kernel boot args before userspace starts, eliminating DHCP as a moving part:
```
ip=172.16.N.2::172.16.N.1:255.255.255.252::eth0:off
```

IP assignments are stored in the `vms` table and are stable across restarts. this also makes caddy routing trivial — you always know the IP for a given VM.

#### NAT and iptables

VMs get outbound internet via masquerade. the host must also block VMs from reaching host services (especially the API server on :3000 and caddy admin API on :2019):

```bash
# outbound internet for VMs
iptables -t nat -A POSTROUTING -s 172.16.0.0/16 -o eth0 -j MASQUERADE
iptables -A FORWARD -i fc-tap-+ -o eth0 -j ACCEPT
iptables -A FORWARD -i eth0 -o fc-tap-+ -m state --state RELATED,ESTABLISHED -j ACCEPT

# CRITICAL: block VMs from reaching host services
iptables -A INPUT -s 172.16.0.0/16 -p tcp --dport 3000 -j DROP   # API server
iptables -A INPUT -s 172.16.0.0/16 -p tcp --dport 2019 -j DROP   # caddy admin API
iptables -A INPUT -s 172.16.0.0/16 -p tcp --dport 22 -j DROP     # host SSH

# (add more DROP rules for any other host-local services)
```

**security note:** if a VM can reach `localhost:2019`, it can rewrite caddy's entire routing config and intercept other users' traffic. the iptables DROP rules above are mandatory, not optional. caddy's admin listener must also be explicitly bound to `127.0.0.1:2019`.

#### TAP device lifecycle

TAP devices require root (or `CAP_NET_ADMIN`) to create and don't clean up on crash. naming convention: `fc-tap-{vm_id}`.

on startup, vm-manager must:
1. enumerate all TAP devices matching `fc-tap-*`
2. compare against VMs in `running` state in the DB
3. delete orphaned TAP devices
4. recreate TAP devices for VMs that should be running

there is a kernel limit on TAP devices (usually 256+), which is fine for our density target but worth monitoring.

### caddy integration

caddy's admin API (`PATCH /config/`) updates routes at runtime without restarts. when a VM starts, api-server writes its route; when it stops, the route is removed.

**important:** caddy's dynamic config is ephemeral — if caddy restarts, all dynamically added routes are lost. therefore, treat caddy config as ephemeral and rebuild all routes from DB state on startup via the reconciliation loop. never rely on caddy persisting dynamic state.

#### wildcard TLS

wildcard cert `*.yourdomain.com` requires DNS-01 ACME challenge, not HTTP-01. this means:
- caddy must be built with the DNS plugin for your provider (cloudflare, route53, etc.) using `xcaddy`
- DNS provider API credentials must be configured in caddy's config
- standard caddy binaries do not include DNS provider plugins

```bash
# build caddy with cloudflare DNS plugin
xcaddy build --with github.com/caddy-dns/cloudflare
```

#### proxy configuration

for VMs running web apps, websockets, SSE, or long-lived connections, set appropriate timeouts in the dynamic route config:

```json
{
  "handle": [{
    "handler": "reverse_proxy",
    "upstreams": [{"dial": "172.16.N.2:PORT"}],
    "flush_interval": -1,
    "transport": {
      "protocol": "http",
      "read_timeout": "300s",
      "write_timeout": "300s"
    }
  }]
}
```

#### stopped VM UX

rather than removing routes when a VM stops (which gives users a confusing 404), keep the route but proxy to a fallback upstream that serves a static "this VM is stopped" page with a link to the dashboard. swap the proxy target on start/stop rather than adding/removing routes.

---

## data model

```sql
-- accounts
CREATE TABLE accounts (
  id TEXT PRIMARY KEY,
  email TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  subscription_status TEXT NOT NULL DEFAULT 'inactive',
  subscription_id TEXT,
  activated_at INTEGER
);

-- sessions (managed by tower-sessions)
CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  expires_at INTEGER NOT NULL
);

-- vms
CREATE TABLE vms (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  name TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'stopped', -- stopped | starting | running | snapshotting | error
  subdomain TEXT UNIQUE NOT NULL,
  vcores INTEGER NOT NULL DEFAULT 2,
  memory_mb INTEGER NOT NULL DEFAULT 2048,
  disk_gb INTEGER NOT NULL DEFAULT 20,
  kernel_path TEXT NOT NULL,
  rootfs_path TEXT NOT NULL,
  snapshot_path TEXT,
  tap_device TEXT,
  ip_address TEXT NOT NULL,
  exposed_port INTEGER NOT NULL DEFAULT 8080,
  pid INTEGER,                  -- firecracker process PID for reconciliation
  socket_path TEXT,             -- unix socket path for this VM's firecracker API
  created_at INTEGER NOT NULL,
  last_started_at INTEGER
);

-- vm_events (audit log)
CREATE TABLE vm_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  vm_id TEXT NOT NULL REFERENCES vms(id),
  event TEXT NOT NULL,  -- started | stopped | snapshot_taken | health_check_failed | etc
  metadata TEXT,        -- json blob
  created_at INTEGER NOT NULL
);

-- processed_webhooks (idempotency)
CREATE TABLE processed_webhooks (
  event_id TEXT PRIMARY KEY,
  processed_at INTEGER NOT NULL
);
```

---

## service structure (monorepo)

```
/
├── crates/
│   ├── api/           -- axum http server, routes, auth middleware
│   ├── vm-manager/    -- firecracker process management, reconciliation, health checks
│   ├── networking/    -- TAP device management, iptables rules, IP allocation
│   ├── router-sync/   -- caddy admin API client, route management
│   ├── db/            -- sqlx queries, migrations
│   └── common/        -- shared types, errors, config
├── frontend/          -- react + tanstack
├── images/            -- curated base rootfs images (alpine, ubuntu-minimal, debian-slim)
├── scripts/           -- dev tooling, provisioning
└── config/
    ├── Caddyfile
    └── config.toml
```

use a cargo workspace. start as a single binary that wires everything together, split later if needed.

the `networking` crate is new — it owns TAP device CRUD, iptables rule management, IP allocation from the CIDR pool, and cleanup of orphaned network resources.

---

## firecracker integration

firecracker exposes a unix socket REST API. you don't need an SDK, the surface area is small:

```rust
// rough client interface — enforces configuration ordering via builder/typestate
impl FirecrackerClient {
    // must be called in this order: machine → boot → drive → network → start
    async fn configure_machine(&self, vcpus: u8, mem_mb: u32) -> Result<()>
    async fn configure_boot_source(&self, kernel: &str, args: &str) -> Result<()>
    async fn configure_drive(&self, path: &str, read_only: bool) -> Result<()>
    async fn configure_network(&self, tap_device: &str, mac: &str) -> Result<()>
    async fn configure_mmds(&self, metadata: &serde_json::Value) -> Result<()>
    async fn start_instance(&self) -> Result<()>
    async fn pause_vm(&self) -> Result<()>
    async fn resume_vm(&self) -> Result<()>
    async fn create_snapshot(&self, snap_path: &str, mem_path: &str) -> Result<()>
    async fn load_snapshot(&self, snap_path: &str, mem_path: &str) -> Result<()>
    async fn shutdown(&self) -> Result<()>
}
```

**important:** the unix socket API has ordering constraints. machine config → boot source → drives → network interfaces → then start. enforce this via a builder pattern or typestate in `FirecrackerClient` so you can't accidentally misconfigure.

### jailer

always run firecracker through the `jailer` binary, which sets up cgroups, seccomp filters, and a chroot for each VM process. this is not optional hardening — it's the baseline security boundary. a firecracker process running as root without the jailer is a direct path to host compromise.

### firecracker metadata service (mmds)

firecracker includes a built-in metadata service at `169.254.169.254` (like AWS's instance metadata). use it to pass config into the guest without modifying the rootfs:
- VM's subdomain
- auth tokens for phoning home to the API
- bootstrap scripts

set this up as part of the firecracker client in phase 1 — it's part of the same API you're already wrapping.

### process management

each VM gets its own firecracker process, its own unix socket, and its own TAP device. vm-manager owns the process lifecycle.

failure modes to handle:
- **orphaned sockets:** left behind after crashes. clean up on startup.
- **zombie processes:** firecracker doesn't always exit cleanly when the guest panics. track PIDs, use `waitpid` with `WNOHANG`, and have a reaping loop.
- **hung shutdowns:** firecracker may hang on shutdown. use `tokio::time::timeout` on every operation — SIGTERM first, then SIGKILL after N seconds.

```rust
// rough shutdown with timeout
async fn stop_vm(&self, vm_id: &str) -> Result<()> {
    let pid = self.get_pid(vm_id)?;
    // try graceful shutdown via firecracker API
    match timeout(Duration::from_secs(10), self.client.shutdown()).await {
        Ok(Ok(())) => {},
        _ => {
            // force kill if graceful shutdown fails
            kill(Pid::from_raw(pid), Signal::SIGKILL)?;
        }
    }
    self.cleanup_socket(vm_id)?;
    self.cleanup_tap(vm_id)?;
    Ok(())
}
```

### vm lifecycle state machine
```
stopped ──start──> starting ──ready──> running
running ──stop───> stopped
running ──snapshot──> snapshotting ──done──> running
running ──pause───> paused ──resume──> running
stopped + snapshot ──restore──> starting ──ready──> running
any ──error──> error ──reset──> stopped
```

### reconciliation loop

the reconciliation loop runs on startup and periodically, syncing the DB with actual host state:

```rust
async fn reconcile(&self) -> Result<()> {
    // 1. scan running firecracker processes on host
    let running_pids = find_firecracker_processes()?;

    // 2. compare against DB
    let db_vms = db::get_vms_by_status("running").await?;

    // 3. kill processes not in DB (leaked/orphaned)
    for pid in running_pids {
        if !db_vms.iter().any(|vm| vm.pid == Some(pid)) {
            kill(Pid::from_raw(pid), Signal::SIGKILL)?;
        }
    }

    // 4. mark VMs as error if their process is gone
    for vm in &db_vms {
        if let Some(pid) = vm.pid {
            if !running_pids.contains(&pid) {
                db::set_vm_status(&vm.id, "error").await?;
            }
        }
    }

    // 5. rebuild TAP devices for VMs that should be running
    // 6. rebuild iptables rules
    // 7. rebuild caddy routes from DB state
    self.rebuild_network_state().await?;
    self.rebuild_caddy_routes().await?;

    Ok(())
}
```

this is critical infrastructure, not a hardening task. without it, any host reboot or API server crash leaves the system in an inconsistent state.

### health check loop

a background loop that pings each running VM to detect crashed guests (the firecracker process might still be alive but the guest is dead):

```rust
async fn health_check_loop(&self) {
    loop {
        for vm in db::get_vms_by_status("running").await? {
            // check if firecracker process is alive
            if !process_alive(vm.pid) {
                db::set_vm_status(&vm.id, "error").await?;
                db::log_event(&vm.id, "health_check_failed", "process_dead").await?;
                continue;
            }
            // optionally: check if guest is responsive via mmds or a simple TCP probe
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
```

### snapshot/restore networking caveat

when restoring a snapshot, the guest's network stack comes back with the state it had at snapshot time (ARP cache, TCP connections, etc.), but the TAP device on the host is new. in-flight connections at snapshot time will be dead. this is fine for our use case. the important thing is to keep IP assignments stable per VM (not per TAP device) — the DB-driven static IP scheme handles this naturally.

---

## routing architecture

### subdomain assignment
on VM create, generate a stable subdomain: `<adjective>-<noun>-<id>.yourdomain.com` or let users pick. stored in `vms.subdomain`.

### caddy config update (on VM start)
```rust
// router-sync patches caddy config at runtime
async fn set_vm_route(subdomain: &str, vm_ip: &str, port: u16) -> Result<()> {
    // PATCH https://localhost:2019/config/apps/http/servers/main/routes
    // sets reverse_proxy for this subdomain → vm_ip:port
    // with flush_interval: -1 and extended timeouts for websocket/SSE
}

async fn set_stopped_route(subdomain: &str) -> Result<()> {
    // proxy to fallback "VM is stopped" static page
}

async fn rebuild_all_routes() -> Result<()> {
    // called on startup and by reconciliation loop
    // reads all VMs from DB, sets routes for running VMs,
    // sets stopped routes for stopped VMs
}
```

wildcard cert `*.yourdomain.com` via caddy's ACME DNS-01 challenge — means zero per-VM cert work but requires the DNS provider plugin (see caddy integration above).

---

## api routes

```
POST   /api/auth/signup       -- create account (argon2 hash password)
POST   /api/auth/login        -- create session
POST   /api/auth/logout       -- destroy session
GET    /api/vms               -- list user's VMs
POST   /api/vms               -- create VM
GET    /api/vms/:id           -- VM details + status
DELETE /api/vms/:id           -- delete VM
POST   /api/vms/:id/start     -- start VM
POST   /api/vms/:id/stop      -- stop VM
POST   /api/vms/:id/snapshot  -- take snapshot
POST   /api/vms/:id/restore   -- restore from snapshot
GET    /api/vms/:id/console   -- websocket console (stretch)
POST   /api/webhooks/lemon    -- lemonsqueezy billing webhook
GET    /api/account            -- account status, quota usage
```

---

## quota enforcement

at VM start time, check account's running resource totals vs limits. **wrap in a serializable transaction** to prevent race conditions where two concurrent start requests both pass the check:

```rust
async fn check_and_reserve_quota(account_id: &str, vm_id: &str, requested: &VmResources) -> Result<()> {
    // use SERIALIZABLE isolation to prevent race conditions
    let mut tx = db::begin().await?;

    let used = db::get_running_vm_resources_tx(&mut tx, account_id).await?;
    let limits = VmLimits { vcores: 8, memory_mb: 12288 };

    if used.vcores + requested.vcores > limits.vcores {
        return Err(QuotaError::VcoresExceeded);
    }
    if used.memory_mb + requested.memory_mb > limits.memory_mb {
        return Err(QuotaError::MemoryExceeded);
    }

    // mark VM as 'starting' inside the transaction to reserve resources
    db::set_vm_status_tx(&mut tx, vm_id, "starting").await?;
    tx.commit().await?;

    Ok(())
}
```

---

## snapshot policy

firecracker snapshots include a full memory dump — a 2GB RAM VM produces a ~2GB snapshot file. enforce limits from the start:

- **max 2 snapshots per VM**
- **max total snapshot storage per account: 20GB**
- on new snapshot, if at limit, require deleting an old one first
- store snapshot metadata in DB, enforce in API layer

---

## base images

ship 3 curated rootfs images:
- **alpine** — minimal, fast boot, good for lightweight services
- **ubuntu-minimal** — familiar, good package ecosystem
- **debian-slim** — middle ground

defer "bring your own rootfs" — validating arbitrary user-supplied disk images opens security and compatibility problems you don't want yet. revisit when users ask for it.

---

## billing flow (lemonsqueezy)

```
1. user signs up → account created, status = 'inactive'
2. frontend redirects to lemonsqueezy checkout
3. LS checkout complete → webhook fires → POST /api/webhooks/lemon
4. webhook handler verifies signature, checks processed_webhooks table, sets account status = 'active'
5. user can now create VMs

on subscription cancel/lapse:
6. webhook fires with cancellation event
7. handler sets status = 'inactive', stops running VMs
```

webhook handler must be idempotent (same event ID = no-op). store processed webhook IDs in `processed_webhooks` table.

---

## build phases

### phase 1: firecracker spike (week 1-2)
- get firecracker running locally with KVM
- **set up jailer from day one** (cgroups, seccomp, chroot)
- write the firecracker HTTP client in rust with builder/typestate pattern
- configure mmds for guest metadata
- boot a basic alpine microVM
- set up TAP networking with static IP assignment (172.16.0.0/16 scheme)
- configure iptables: NAT masquerade for outbound, DROP rules for host services
- SSH into VM from host

**done when:** `ssh user@172.16.0.2` works, VM has outbound internet, VM cannot reach host :3000/:2019

### phase 2: routing (week 2-3)
- build caddy with xcaddy + DNS provider plugin
- get wildcard cert via DNS-01 challenge (use staging cert for dev)
- write the caddy admin API client
- wire up subdomain → VM IP routing with websocket/SSE-friendly timeouts
- implement stopped-VM fallback page
- test `curl https://<vmid>.yourdomain.com` hits the VM

**done when:** can curl a subdomain and hit a process inside the VM, stopped VM shows fallback page

### phase 3: vm lifecycle api + reconciliation (week 3-4)
- cargo workspace, db crate, sqlx migrations
- `networking` crate for TAP device and iptables management
- vm-manager with state machine
- **reconciliation loop** (sync DB ↔ host state on startup and periodically)
- **health check loop** (detect dead guests)
- axum api server wired to vm-manager
- create/start/stop/delete via api calls

**done when:** can `curl -X POST /api/vms && curl -X POST /api/vms/:id/start` and a VM boots. killing a firecracker process results in the VM being marked as `error` within 30s.

### phase 4: snapshot/restore (week 4-5)
- implement firecracker snapshot API calls
- store snapshot paths + metadata in db
- enforce snapshot limits (2 per VM, 20GB per account)
- test restore from snapshot preserves state
- verify networking survives snapshot/restore cycle (stable IPs)

**done when:** snapshot + restore round-trip works, process state survives, networking comes back clean

### phase 5: auth + accounts (week 5-6)
- axum-login + tower-sessions + argon2 integration
- account creation flow
- api routes protected by auth middleware
- quota checking on VM operations (with serializable transaction)

**done when:** signup → login → create VM flow works end to end, concurrent start requests don't exceed quota

### phase 6: frontend (week 6-8)
- tanstack router + query setup
- auth pages (signup, login)
- vm list dashboard
- vm detail page (status, start/stop/snapshot buttons)
- basic account/quota display
- snapshot management (list, delete, restore)

**done when:** full flow works in browser without curl

### phase 7: billing (week 8-9)
- lemonsqueezy product + webhook setup
- webhook handler in axum with idempotency (processed_webhooks table)
- account activation flow
- inactive account enforcement

**done when:** can pay, get activated, create vms, cancel and get deactivated

### phase 8: hardening (week 9-10)
- structured logging (tracing crate)
- rate limiting on api
- basic monitoring (prometheus metrics endpoint)
- audit orphaned resource cleanup (TAP devices, sockets, firecracker processes)
- document operational runbook (what to do when things break)

**note:** most of the items that were previously in this phase (process reconciliation, jailer setup, iptables rules, health checks, snapshot limits) have been moved to earlier phases where they belong.

---

## local dev setup

```bash
# verify KVM is available
ls /dev/kvm && ls /dev/net/tun

# firecracker binary + jailer
curl -Lo firecracker https://github.com/firecracker-microvm/firecracker/releases/download/v1.9.0/firecracker-v1.9.0-x86_64
curl -Lo jailer https://github.com/firecracker-microvm/firecracker/releases/download/v1.9.0/jailer-v1.9.0-x86_64
chmod +x firecracker jailer

# kernel + rootfs for testing
# firecracker team provides pre-built ones:
# https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/v1.9/x86_64/vmlinux-6.1.102
# https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/v1.9/x86_64/ubuntu-22.04.ext4

# caddy (built with DNS plugin)
go install github.com/caddyserver/xcaddy/cmd/xcaddy@latest
xcaddy build --with github.com/caddy-dns/cloudflare
```

for local dev, skip real TLS and just use caddy with HTTP + /etc/hosts entries for `*.localvm.dev`. still set up iptables rules locally to catch networking bugs early.

---

## things to defer

- custom domains (cert management complexity)
- vm console over websocket (nice, not essential)
- teams / shared access
- vm marketplace / templates UI
- metrics graphs in frontend (just show status for now)
- multiple host nodes (single host is fine to start)
- bring-your-own-rootfs (ship curated images first)

---

## decisions made (previously open questions)

- **vm default images**: ship 3 curated rootfs images (alpine, ubuntu-minimal, debian-slim). defer user-supplied rootfs.
- **storage for snapshots**: max 2 snapshots per VM, 20GB total per account. enforce in API from launch.
- **vm port exposure**: single port per VM, user-configurable (stored in DB as `exposed_port`). users can run a reverse proxy inside their VM if they need more. defer `<port>-<vmid>` routing.
- **host oversubscription**: soft cap at 10 customers/host. monitor actual usage before launch and adjust.

## remaining open questions

- **DNS provider**: which provider for caddy's DNS-01 challenge? cloudflare is the most common choice with the best xcaddy plugin support.
- **monitoring/alerting**: what to use for host-level monitoring? prometheus + grafana is the obvious choice but may be overkill for a single host. consider a simpler solution like a healthcheck endpoint + uptime service.
- **backup strategy**: how to back up the postgres DB and snapshot storage? rsync to a second machine? automated hetzner snapshots of the host?
