# phase 4: snapshot/restore

**goal:** take a firecracker snapshot of a running VM, store it, and restore from it. process state survives the round-trip. networking comes back clean.

**done when:**
- `POST /api/vms/:id/snapshot` pauses VM, writes snapshot+mem files, resumes VM
- `POST /api/vms/:id/restore` boots a new firecracker process from saved snapshot files
- guest process state (e.g. a running counter) survives snapshot → restore
- networking works after restore (can ping guest, caddy route active)
- snapshot limits enforced: max 2 per VM, VM must be stopped to restore

---

## fctools snapshot API (already researched)

```rust
// 1. pause the running VM
vm.pause().await?;  // VM must be Running

// 2. create snapshot (VM must be Paused)
let snapshot: VmSnapshot = vm.create_snapshot(CreateSnapshot {
    snapshot_type: Some(SnapshotType::Full),
    snapshot: snapshot_resource,   // Resource pointing to snap file path
    mem_file: mem_resource,        // Resource pointing to mem file path
}).await?;
// snapshot.snapshot_path — actual path written
// snapshot.mem_file_path — actual path written
// snapshot.configuration_data — original VM config (needed for restore)

// 3. resume VM
vm.resume().await?;

// --- restore ---

// prepare_vm consumes the VmSnapshot and wires up a new VM process
let new_vm = snapshot.prepare_vm(&mut old_vm, PrepareVmFromSnapshotOptions {
    executor,
    process_spawner: DirectProcessSpawner,
    runtime: TokioRuntime,
    moved_resource_type: MovedResourceType::HardLinkedOrCopied,
    ownership_model: VmmOwnershipModel::Shared,
    track_dirty_pages: Some(true),
    resume_vm: Some(true),   // auto-resume after load
    network_overrides: vec![NetworkOverride {
        iface_id: "eth0".into(),
        host_dev_name: tap.name.clone(),  // new TAP device
    }],
}).await?;
new_vm.start(Duration::from_secs(10)).await?;
```

key points:
- `prepare_vm` takes `&mut old_vm` — the old VM must still be alive (use for restore-to-same-slot or keep a placeholder)
- `network_overrides` rewires the TAP device — always provide this so the restored VM uses the freshly allocated TAP
- `resume_vm: Some(true)` means firecracker auto-resumes without a separate API call after load
- snapshot files are regular files on disk — manage their paths explicitly

---

## what to build

### db: snapshot table + functions

new migration `0002_snapshots.sql`:
```sql
CREATE TABLE snapshots (
    id TEXT PRIMARY KEY,
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    label TEXT,                        -- optional user label
    snapshot_path TEXT NOT NULL,       -- path to .snap file
    mem_path TEXT NOT NULL,            -- path to .mem file
    size_bytes BIGINT NOT NULL DEFAULT 0,
    created_at BIGINT NOT NULL
);
```

new db functions:
```rust
pub async fn create_snapshot(pool, snap: &NewSnapshot) -> Result<SnapshotRow>
pub async fn get_snapshot(pool, id: &str) -> Result<Option<SnapshotRow>>
pub async fn list_snapshots(pool, vm_id: &str) -> Result<Vec<SnapshotRow>>
pub async fn delete_snapshot(pool, id: &str) -> Result<()>
pub async fn count_snapshots(pool, vm_id: &str) -> Result<i64>
```

### api routes

add to `crates/api/src/lib.rs`:
```
POST   /api/vms/:id/snapshot            -- take snapshot (VM must be running)
GET    /api/vms/:id/snapshots           -- list snapshots for a VM
DELETE /api/vms/:id/snapshots/:snap_id  -- delete a snapshot
POST   /api/vms/:id/restore/:snap_id    -- restore from snapshot (VM must be stopped)
```

snapshot response:
```rust
struct SnapshotResponse {
    id: String,
    vm_id: String,
    label: Option<String>,
    size_bytes: i64,
    created_at: i64,
}
```

### VmOps trait additions

```rust
// in api/src/lib.rs VmOps trait:
async fn take_snapshot(&self, vm_id: &str, label: Option<String>) -> anyhow::Result<db::SnapshotRow>;
async fn restore_snapshot(&self, vm_id: &str, snap_id: &str) -> anyhow::Result<()>;
async fn list_snapshots(&self, vm_id: &str) -> anyhow::Result<Vec<db::SnapshotRow>>;
async fn delete_snapshot(&self, vm_id: &str, snap_id: &str) -> anyhow::Result<()>;
```

### manager.rs additions

**snapshot storage path:**
```rust
fn snapshot_dir(vm_id: &str) -> PathBuf {
    PathBuf::from(format!("/var/lib/spwn/snapshots/{vm_id}"))
}
// files: {dir}/{snap_id}.snap and {dir}/{snap_id}.mem
```

**`take_snapshot`:**
```rust
pub async fn take_snapshot(&self, vm_id: &str, label: Option<String>) -> anyhow::Result<SnapshotRow> {
    // 1. check VM is running
    // 2. enforce limit: count_snapshots <= 1 (max 2, so reject if already at 2)
    // 3. set status "snapshotting"
    // 4. get fc_vm handle from self.running
    // 5. vm.pause()
    // 6. create resource handles for snap/mem paths
    // 7. vm.create_snapshot(...)
    // 8. vm.resume()
    // 9. set status back to "running"
    // 10. measure file sizes, write to DB
}
```

**`restore_snapshot`:**
```rust
pub async fn restore_snapshot(&self, vm_id: &str, snap_id: &str) -> anyhow::Result<()> {
    // 1. get VM from DB, check status == "stopped"
    // 2. get snapshot row from DB
    // 3. set status "starting"
    // 4. allocate TAP device
    // 5. create a minimal placeholder Vm to pass to prepare_vm
    //    (or: use Vm::prepare with RestoredFromSnapshot config)
    // 6. call snapshot.prepare_vm with network_overrides for new TAP
    // 7. new_vm.start(timeout)
    // 8. find PID, update DB to running
    // 9. set caddy route
    // 10. insert into self.running
}
```

note on `prepare_vm`: it takes `&mut old_vm` — for restore from stopped VM there is no old_vm. check fctools docs/source for whether there's a `Vm::prepare` path that accepts a `RestoredFromSnapshot` config variant instead. if so, use that. if not, create a dummy Vm from a fresh prepare and pass it.

actually — look at `VmConfiguration::RestoredFromSnapshot`:
```rust
VmConfiguration::RestoredFromSnapshot {
    load_snapshot: LoadSnapshot { ... },
    data: VmConfigurationData { ... },  // must match original config
}
```
this may be the cleaner path: `Vm::prepare` with the restored config, no need for prepare_vm at all.

### snapshot limit enforcement

```rust
const MAX_SNAPSHOTS_PER_VM: i64 = 2;

// in take_snapshot, before doing anything:
let count = db::count_snapshots(&self.pool, vm_id).await?;
if count >= MAX_SNAPSHOTS_PER_VM {
    return Err(anyhow!("snapshot limit reached ({MAX_SNAPSHOTS_PER_VM} max). delete one first."));
}
```

no disk quota check in phase 4 — add in phase 5 with the rest of quota enforcement.

### snapshot storage directory

create on startup:
```rust
// in main.rs
std::fs::create_dir_all("/var/lib/spwn/snapshots")?;
```

or make it configurable via `SNAPSHOT_DIR` env var.

### delete_vm cleanup

when a VM is deleted, its snapshot files and DB rows should be cleaned up:
```rust
// in delete_vm:
let snaps = db::list_snapshots(&self.pool, id).await?;
for snap in snaps {
    std::fs::remove_file(&snap.snapshot_path).ok();
    std::fs::remove_file(&snap.mem_path).ok();
}
// ON DELETE CASCADE handles DB rows
```

---

## dependencies to add

```toml
# no new crate deps needed — snapshot functionality is in fctools already
# just need the "vm" feature which is already enabled
```

make sure `VmSnapshot`, `CreateSnapshot`, `SnapshotType`, `PrepareVmFromSnapshotOptions`, `NetworkOverride` are imported from `fctools::vm`.

---

## verification checklist

```bash
# start a VM
curl -X POST http://localhost:3019/api/vms/$ID/start

# SSH in and run something stateful
ssh -i /tmp/ubuntu.id_rsa root@172.16.1.2 "nohup sh -c 'i=0; while true; do echo \$i > /tmp/counter; i=\$((i+1)); sleep 1; done' &"
ssh -i /tmp/ubuntu.id_rsa root@172.16.1.2 "cat /tmp/counter"  # e.g. 5

# take snapshot
curl -X POST http://localhost:3019/api/vms/$ID/snapshot \
  -H "Content-Type: application/json" \
  -d '{"label":"test-snap"}'
# → {"id":"snap-abc","size_bytes":...}

# stop VM
curl -X POST http://localhost:3019/api/vms/$ID/stop

# restore from snapshot
curl -X POST http://localhost:3019/api/vms/$ID/restore/snap-abc

# poll until running
curl http://localhost:3019/api/vms/$ID
# → {"status":"running"}

# verify state survived (counter should be close to where we left off)
ssh -i /tmp/ubuntu.id_rsa root@172.16.1.2 "cat /tmp/counter"  # e.g. 6 or 7

# verify networking
curl -H "Host: vivid-moon-be33:8080" http://localhost:8080/

# try to take a 3rd snapshot → should fail
curl -X POST http://localhost:3019/api/vms/$ID/snapshot
# → 500 snapshot limit reached
```

---

## what's NOT in scope for phase 4

- snapshot disk quota (phase 5 with full quota enforcement)
- diff snapshots (fctools has `SnapshotType::Diff` behind a feature flag — skip for now)
- snapshot export/import across hosts
- websocket console
- auth (phase 5)
