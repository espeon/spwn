# phase 5: control plane + host agent split

**goal:** split `vm-manager` into `host-agent` (one per physical host, manages local firecracker VMs) and `control-plane` (global, external API + scheduling). both can run on one machine for single-node dev. all internal communication uses gRPC (tonic), which also supports the streaming needed for SSH/console later.

**done when:**

- `host-agent` binary manages local VMs, registers with control plane, streams events via gRPC
- `control-plane` binary handles external API, schedules VMs onto hosts, proxies lifecycle ops via gRPC
- single-node dev works: both on same machine, agent on :4000, control plane on :3000
- `vms` table has `host_id` — every VM pinned to the agent that created it
- `hosts` table tracks registered agents with address + capacity
- a second machine can join by running `host-agent` pointed at the same postgres + control plane

---

## crate layout after this phase

```
crates/db               unchanged
crates/api              unchanged — external API trait + axum router (used by control-plane)
crates/networking       unchanged — used by host-agent only
crates/router-sync      unchanged — used by control-plane only
crates/common           unchanged

crates/agent-proto      NEW — .proto definitions + tonic generated code (shared by both)
crates/host-agent       NEW — replaces vm-manager; local firecracker management + gRPC server
crates/control-plane    NEW — external API + scheduler + gRPC client to agents
```

`crates/vm-manager` is deleted.

---

## agent-proto crate

contains the `.proto` file and a `build.rs` that runs `tonic-build`. both `host-agent` and `control-plane` depend on this crate.

```protobuf
// proto/agent.proto
syntax = "proto3";
package agent;

service HostAgent {
  // lifecycle — unary
  rpc StartVm(StartVmRequest)           returns (StartVmResponse);
  rpc StopVm(StopVmRequest)             returns (StopVmResponse);
  rpc TakeSnapshot(SnapshotRequest)     returns (SnapshotResponse);
  rpc RestoreSnapshot(RestoreRequest)   returns (RestoreResponse);

  // agent → control plane event stream (server-streaming)
  // control plane calls this once per agent; agent pushes events as they happen
  rpc WatchEvents(WatchRequest)         returns (stream AgentEvent);

  // bidirectional console/SSH stream (reserved for phase 6)
  rpc StreamConsole(stream ConsoleInput) returns (stream ConsoleOutput);

  // registration + heartbeat
  rpc Register(RegisterRequest)         returns (RegisterResponse);
  rpc Heartbeat(HeartbeatRequest)       returns (HeartbeatResponse);
}

message StartVmRequest  { string vm_id = 1; }
message StartVmResponse { bool ok = 1; string error = 2; }

message StopVmRequest   { string vm_id = 1; }
message StopVmResponse  { bool ok = 1; string error = 2; }

message SnapshotRequest  { string vm_id = 1; string snap_id = 2; string snapshot_path = 3; string mem_path = 4; }
message SnapshotResponse { bool ok = 1; string error = 2; int64 size_bytes = 3; }

message RestoreRequest  { string vm_id = 1; string snap_id = 2; }
message RestoreResponse { bool ok = 1; string error = 2; }

message WatchRequest {}

message AgentEvent {
  string vm_id     = 1;
  string event     = 2;  // "started" | "stopped" | "crashed" | "snapshot_taken"
  string detail    = 3;  // optional human-readable detail
  int64  timestamp = 4;
}

message ConsoleInput  { bytes data = 1; }
message ConsoleOutput { bytes data = 1; }

message RegisterRequest {
  string host_id       = 1;
  string name          = 2;
  string address       = 3;  // gRPC address control plane uses to reach this agent
  uint32 vcpu_total    = 4;
  uint32 mem_total_mb  = 5;
}
message RegisterResponse { bool ok = 1; }

message HeartbeatRequest {
  string host_id      = 1;
  repeated string running_vm_ids = 2;
  uint32 vcpu_used    = 3;
  uint32 mem_used_mb  = 4;
}
message HeartbeatResponse {}
```

`build.rs`:
```rust
fn main() {
    tonic_build::compile_protos("proto/agent.proto").unwrap();
}
```

dependencies:
```toml
[dependencies]
tonic = "0.12"
prost = "0.13"

[build-dependencies]
tonic-build = "0.12"
```

---

## db changes

### migration `0005_hosts.sql`

```sql
CREATE TABLE IF NOT EXISTS hosts (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    address      TEXT NOT NULL,      -- grpc address, e.g. http://10.0.0.1:4000
    vcpu_total   INTEGER NOT NULL DEFAULT 0,
    mem_total_mb INTEGER NOT NULL DEFAULT 0,
    last_seen_at BIGINT NOT NULL DEFAULT 0
);

ALTER TABLE vms ADD COLUMN IF NOT EXISTS host_id TEXT REFERENCES hosts(id);
```

### new db functions

```rust
pub async fn upsert_host(pool, host: &NewHost) -> Result<HostRow>
pub async fn update_host_heartbeat(pool, id: &str, vcpu_used: i32, mem_used_mb: i32, now: i64) -> Result<()>
pub async fn list_hosts(pool) -> Result<Vec<HostRow>>
pub async fn get_host(pool, id: &str) -> Result<Option<HostRow>>
pub async fn set_vm_host(pool, vm_id: &str, host_id: &str) -> Result<()>
```

---

## host-agent binary

essentially `vm-manager` with the external API stripped out and replaced by a gRPC server.

**startup sequence:**
1. load env / config
2. setup networking (iptables, ip forwarding) — same as now
3. connect to postgres
4. run migrations (agent runs them too so single-node works without a separate control plane boot order)
5. reconcile local VMs
6. connect to control plane gRPC, call `Register`
7. start gRPC server (tonic) on `AGENT_LISTEN_ADDR`
8. start heartbeat loop (every 10s, call `Heartbeat` with current load)
9. open `WatchEvents` stream — push events to control plane as VMs start/stop/crash

**gRPC handlers** (move from `VmManager` impl):
- `StartVm` → `manager.start_vm_inner`
- `StopVm` → `manager.stop_vm`
- `TakeSnapshot` → `manager.take_snapshot`
- `RestoreSnapshot` → `manager.restore_snapshot`
- `StreamConsole` → stub returning unimplemented (phase 6)

reconciler + health check loops stay on the agent — they interact with local processes.

**env vars:**

| var | default | description |
|-----|---------|-------------|
| `HOST_ID` | persisted UUID in `/var/lib/spwn/host-id` | unique id for this host |
| `HOST_NAME` | system hostname | human-readable label |
| `AGENT_LISTEN_ADDR` | `0.0.0.0:4000` | gRPC listen address |
| `AGENT_PUBLIC_ADDR` | required | address control plane uses to reach this agent |
| `CONTROL_PLANE_URL` | required | gRPC address of control plane for registration |
| `DATABASE_URL` | — | postgres |
| `KERNEL_PATH` | required | vmlinux path |
| `IMAGES_DIR` | `/var/lib/spwn/images` | squashfs images |
| `OVERLAY_DIR` | `/var/lib/spwn/overlays` | per-VM ext4 overlays |
| `SNAPSHOT_DIR` | `/var/lib/spwn/snapshots` | snapshot files |

---

## control-plane binary

takes over the external-facing role from `vm-manager`.

**implements `VmOps` trait** — each method:
1. looks up `vm.host_id` in postgres
2. looks up the host's gRPC address
3. creates a tonic client, calls the appropriate RPC
4. updates postgres with the result

**scheduling** (`create_vm`):
1. pick host: query `hosts` for agents with heartbeat in last 30s, pick highest free memory
2. set `vm.host_id`
3. call agent `StartVm` (or just write the row — agent reconciler will pick it up)

**event stream consumer:**
- on startup, for each known host, open a `WatchEvents` stream
- when an `AgentEvent` arrives, write to `vm_events` table and update VM status
- reconnects with backoff if stream drops

**host registration endpoint:**
- agents call the control plane's gRPC server to `Register` and send `Heartbeat`
- control plane is also a gRPC server, not just a client

**env vars:**

| var | default | description |
|-----|---------|-------------|
| `LISTEN_ADDR` | `0.0.0.0:3000` | external HTTP API |
| `GRPC_LISTEN_ADDR` | `0.0.0.0:5000` | gRPC listen (for agent registration/heartbeat) |
| `DATABASE_URL` | — | postgres |
| `CADDY_URL` | `http://localhost:2019` | caddy admin API |
| `STATIC_FILES_PATH` | `/var/lib/spwn/static` | caddy static files |

---

## single-node dev setup

```bash
# terminal 1 — control plane
LISTEN_ADDR=0.0.0.0:3000 GRPC_LISTEN_ADDR=0.0.0.0:5000 \
DATABASE_URL=postgres://... CADDY_URL=http://localhost:2019 \
./target/debug/spwn-control-plane

# terminal 2 — host agent
AGENT_PUBLIC_ADDR=http://localhost:4000 \
AGENT_LISTEN_ADDR=0.0.0.0:4000 \
CONTROL_PLANE_URL=http://localhost:5000 \
KERNEL_PATH=/tmp/vmlinux IMAGES_DIR=/var/lib/spwn/images \
DATABASE_URL=postgres://... \
sudo -E ./target/debug/spwn-host-agent
```

---

## what moves where

| current location | moves to |
|-----------------|----------|
| `manager.rs` start/stop/snapshot/restore | `host-agent` gRPC handlers |
| `manager.rs` create_vm / delete_vm (VmOps) | `control-plane` VmOps impl |
| `reconcile.rs` | `host-agent` |
| `health.rs` | `host-agent` |
| `subdomain.rs` | `control-plane` |
| `caddy.set_vm_route` | `control-plane` (triggered by WatchEvents) |
| `main.rs` iptables setup | `host-agent` main |
| `crates/api` router | `control-plane` main |

---

## what's NOT in scope for phase 5

- `StreamConsole` / SSH proxying (phase 6)
- TLS on internal gRPC (add when hosts are on separate machines)
- VM migration between hosts
- snapshot transfer between hosts
- auth / API keys (phase 6)
- bin-packing / preemption scheduling
