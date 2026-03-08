# phase 3: vm lifecycle api + reconciliation

**goal:** full VM CRUD via HTTP API, backed by postgres. reconciliation loop syncs DB ↔ host state. health check loop detects dead guests.

**done when:**
- `POST /api/vms` + `POST /api/vms/:id/start` boots a VM
- `POST /api/vms/:id/stop` stops it cleanly
- killing a firecracker process manually → VM marked `error` within 30s
- host reboot → reconciliation loop cleans up orphans and marks stale VMs as `error`
- jailer is used for all VM processes (not `UnrestrictedVmmExecutor`)

---

## what to build

### `crates/db/src/lib.rs`

sqlx queries + migrations. use sqlx with compile-time checked queries (`query!` macro) against a test DB in CI, or `query_as!` with explicit types if you want to skip the offline mode setup for now.

**migrations** (in `crates/db/migrations/`):
```
0001_init.sql  -- accounts, sessions, vms, vm_events, processed_webhooks
```

schema is in plan-v2.md data model section. copy it verbatim.

key functions to implement:
```rust
pub async fn create_vm(pool: &PgPool, vm: &NewVm) -> Result<()>
pub async fn get_vm(pool: &PgPool, id: &str) -> Result<Option<VmRow>>
pub async fn list_vms(pool: &PgPool, account_id: &str) -> Result<Vec<VmRow>>
pub async fn set_vm_status(pool: &PgPool, id: &str, status: &str) -> Result<()>
pub async fn set_vm_pid(pool: &PgPool, id: &str, pid: Option<i64>) -> Result<()>
pub async fn get_vms_by_status(pool: &PgPool, status: &str) -> Result<Vec<VmRow>>
pub async fn log_event(pool: &PgPool, vm_id: &str, event: &str, metadata: Option<&str>) -> Result<()>
pub async fn begin(pool: &PgPool) -> Result<Transaction<Postgres>>
```

### `crates/vm-manager` — refactor spike into proper modules

```
crates/vm-manager/src/
├── main.rs          -- binary entry point, wires everything together
├── manager.rs       -- VmManager struct, start_vm / stop_vm / get_status
├── reconcile.rs     -- reconciliation loop
├── health.rs        -- health check loop
└── jailer.rs        -- jailer argument construction
```

**`manager.rs`** — core VM lifecycle:
```rust
pub struct VmManager {
    db: PgPool,
    networking: NetworkManager,
    caddy: CaddyClient,
    installation: VmmInstallation,
}

impl VmManager {
    pub async fn start_vm(&self, vm_id: &str) -> Result<()>
    // 1. get vm from DB
    // 2. allocate TAP device
    // 3. spawn firecracker via jailer executor
    // 4. Vm::prepare + vm.start
    // 5. set_vm_status "running", set_vm_pid
    // 6. caddy: set_vm_route

    pub async fn stop_vm(&self, vm_id: &str) -> Result<()>
    // 1. vm.shutdown (CtrlAltDel → Kill sequence)
    // 2. vm.cleanup
    // 3. release TAP device
    // 4. set_vm_status "stopped", clear pid
    // 5. caddy: set_stopped_route

    pub async fn delete_vm(&self, vm_id: &str) -> Result<()>
    // must be stopped first. delete from DB, clean up snapshot files.
}
```

**jailer setup** — switch spike from `UnrestrictedVmmExecutor` to `JailedVmmExecutor`:
```rust
// jailer needs uid/gid to drop to. use a dedicated "firecracker" user.
// adduser --system --no-create-home --group firecracker
let jailer_args = JailerArguments::new(uid, gid, vm_id.as_str());
let executor = JailedVmmExecutor::new(vmm_args, jailer_args);
```

note: jailer chroots the FC process, so kernel/rootfs paths passed to fctools must be absolute paths that the jailer will bind-mount into the chroot. fctools's `ResourceSystem` with `Moved` resources handles this — it copies/hard-links files into the jailer's chroot directory before launch.

**`reconcile.rs`:**
```rust
pub async fn run_reconciliation(manager: Arc<VmManager>) -> ! {
    loop {
        if let Err(e) = reconcile_once(&manager).await {
            tracing::error!("reconciliation error: {e}");
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

async fn reconcile_once(manager: &VmManager) -> Result<()> {
    // 1. find all firecracker PIDs on host (scan /proc for "firecracker" in cmdline)
    // 2. get all DB vms with status="running"
    // 3. kill PIDs not tracked in DB
    // 4. for tracked VMs whose PID is gone → set_vm_status "error" + log_event
    // 5. clean up orphaned TAP devices (list_tap_devices vs DB)
    // 6. clean up orphaned sockets (/tmp/fc-*.sock not in DB)
    // 7. rebuild caddy routes from DB state
}
```

scan `/proc` for firecracker processes:
```rust
fn find_firecracker_pids() -> Result<Vec<i32>> {
    // read /proc/*/cmdline, filter entries containing "firecracker"
    // return as Vec<i32>
}
```

**`health.rs`:**
```rust
pub async fn run_health_checks(manager: Arc<VmManager>) -> ! {
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        for vm in manager.db.get_vms_by_status("running").await.unwrap_or_default() {
            if let Some(pid) = vm.pid {
                // check if /proc/{pid} exists
                if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                    manager.db.set_vm_status(&vm.id, "error").await.ok();
                    manager.db.log_event(&vm.id, "health_check_failed", Some("process_dead")).await.ok();
                }
            }
        }
    }
}
```

### `crates/api/src/lib.rs` — axum server

routes for phase 3:
```
GET    /api/vms               -> list_vms handler
POST   /api/vms               -> create_vm handler
GET    /api/vms/:id           -> get_vm handler
DELETE /api/vms/:id           -> delete_vm handler
POST   /api/vms/:id/start     -> start_vm handler
POST   /api/vms/:id/stop      -> stop_vm handler
```

auth middleware is a stub for now — always passes. real auth is phase 5.

`VmManager` is passed as axum `State`. start/stop operations are async — return 202 Accepted and let the operation complete in the background (update status via DB). the client polls `GET /api/vms/:id` to watch status.

subdomain generation on create: `<adj>-<noun>-<6 char hex>` using a small wordlist. check for uniqueness in DB before committing.

### main binary wiring

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:spwn@localhost/spwn".to_string());
    let pool = PgPool::connect(&database_url).await?;
    sqlx::migrate!("migrations").run(&pool).await?;

    let manager = Arc::new(VmManager::new(pool.clone(), ...));

    // run reconciliation on startup (not in background loop yet)
    reconcile_once(&manager).await?;

    // spawn background tasks
    tokio::spawn(run_reconciliation(manager.clone()));
    tokio::spawn(run_health_checks(manager.clone()));

    // start API server
    let app = api::router(manager.clone());
    axum::serve(TcpListener::bind("0.0.0.0:3000").await?, app).await?;

    Ok(())
}
```

---

## dependencies to add

```toml
# db/Cargo.toml
sqlx = { version = "0.8", features = ["postgres", "runtime-tokio-native-tls", "migrate", "macros"] }
tokio = { version = "1", features = ["full"] }
thiserror = "2"
common = { path = "../common" }

# api/Cargo.toml
axum = { version = "0.8", features = ["macros"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
common = { path = "../common" }
db = { path = "../db" }

# vm-manager/Cargo.toml (additions)
db = { path = "../db" }
router-sync = { path = "../router-sync" }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

---

## verification checklist

```bash
# create a VM
curl -X POST http://localhost:3000/api/vms \
  -H "Content-Type: application/json" \
  -d '{"name":"test","vcores":2,"memory_mb":512}'
# → {"id":"abc123","subdomain":"quick-fox-abc123","status":"stopped",...}

# start it
curl -X POST http://localhost:3000/api/vms/abc123/start
# → 202 Accepted

# poll until running
curl http://localhost:3000/api/vms/abc123
# → {"status":"running",...}

# verify networking (TAP device + routing)
ip addr show fc-tap-abc123
ping -c 1 172.16.0.2

# kill the firecracker process manually
sudo kill -9 $(pgrep firecracker)

# within 30s, health check should mark it as error
curl http://localhost:3000/api/vms/abc123
# → {"status":"error",...}
```

---

## what's NOT in scope for phase 3

- auth (phase 5) — API is open in phase 3
- quota enforcement (phase 5)
- snapshot/restore (phase 4)
- billing (phase 7)
