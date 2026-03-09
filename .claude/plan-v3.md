# spwn — build plan (v3)

## what we've built

a firecracker-based VM platform where users get a fixed resource pool (8 vcores, 12gb ram, 5 vms), persistent microVMs with overlayfs, snapshot/restore, wildcard subdomain routing via caddy, session auth with invite codes, and a react frontend.

### completed phases

- **phase 1** — firecracker spike: TAP networking, iptables, static IP scheme, boot a VM, SSH in
- **phase 2** — caddy routing: wildcard TLS, subdomain→VM proxy, stopped-VM fallback page
- **phase 3** — VM lifecycle API + reconciliation: postgres, CRUD, state machine, health checks, reconciler
- **phase 4** — snapshot/restore + overlayfs: shared squashfs base, per-VM ext4 overlay, full snapshot round-trip
- **phase 4b** — control plane + host agent split: gRPC (tonic), agent registration, heartbeat, event streaming
- **phase 5** — auth + accounts: argon2, session cookies, invite codes, quota enforcement (serializable tx)
- **phase 6** — frontend: react + tanstack router/query, auth flows, VM dashboard, snapshot management, theme picker, account settings

### current crate layout

```
crates/common          shared types (VmId, etc.)
crates/networking      TAP devices, iptables, IP allocation
crates/db              sqlx/postgres queries + migrations
crates/auth            password hashing (argon2), session extractor, auth routes
crates/api             axum router + VmOps trait
crates/router-sync     caddy admin API client
crates/agent-proto     .proto + tonic generated code
crates/host-agent      firecracker process management + gRPC server
crates/control-plane   external HTTP API + scheduler + gRPC client to agents
frontend/              react + tanstack router/query (vite, pnpm)
```

---

## what's next — the fun stuff

the platform works. a user can sign up, create VMs, start/stop/snapshot them, and hit them over HTTPS subdomains. now we make it a place people actually want to hang out.

priorities:

1. **CLI + SSH** — meet users where they live (the terminal)
2. **SSH forwarding + routing** — expose VM ports without caddy, SSH tunnels as first-class citizens
3. **proper multi-node** — real scheduling, host failure handling, not just "it works on one box"
4. **proper testing** — cargo-nextest, integration test harness, CI that catches regressions
5. **playful features** — make it feel like a playground, not a cloud provider

billing comes eventually. it's not blocking anything.

---

## tech additions

| layer                 | choice                                 | why                                                                           |
| --------------------- | -------------------------------------- | ----------------------------------------------------------------------------- |
| CLI                   | rust (clap)                            | single binary, same workspace, dogfood the API                                |
| SSH gateway           | Go (wish + bubbletea + lip gloss)      | purpose-built SSH app server, interactive TUI per connection, charm ecosystem |
| SSH ↔ control plane   | gRPC (protobuf)                        | reuse agent-proto patterns, streaming for console relay                       |
| test runner           | cargo-nextest                          | parallel, better output, partitioned CI                                       |
| multi-node scheduling | consistent hashing + pg advisory locks | simple, no etcd/raft dependency                                               |
| VM console            | SSH → bubbletea TUI → gRPC → VM        | real terminal with interactive UI, not a dumb shell relay                     |

---

## system architecture (v3)

```
                          internet
                             |
                    ┌────────┴────────┐
                    |                 |
                  caddy          ssh gateway
              (HTTPS :443)       (TCP :2222)
              wildcard TLS       Go sidecar
                    |            (wish/bubbletea)
              ┌─────┴──────┐         |
              |            |      gRPC
         control-plane   VM    ┌────┴────┐
         (HTTP :3019)    HTTP  |         |
         (gRPC :5000)       control   host
              |             plane    agents
     ┌────────┼────────┐
     |        |        |
   agent    agent    agent
   host-1   host-2   host-N
     |        |        |
   FC VMs   FC VMs   FC VMs
```

the SSH gateway is a Go sidecar using charmbracelet's wish (SSH server) + bubbletea (TUI framework) + lip gloss (styling). each SSH connection gets a full interactive TUI — not a dumb terminal relay. the gateway talks to the control plane and host agents via gRPC.

it handles:

- **interactive TUI** — VM dashboard, status, start/stop directly from the SSH session
- **shell relay** — drop into a VM's shell (gateway → gRPC → host agent → VM)
- **TCP port forwarding** — SSH `-L` and `-R` forwarding, proxied through the gateway
- **SCP/SFTP** — file transfer passthrough to VMs

---

## phase 7: CLI

**goal:** a `spwn` CLI that talks to the control plane API. every action available in the frontend should be doable from the terminal.

### crate: `crates/cli`

```
crates/cli/
  Cargo.toml
  src/
    main.rs        entry point, clap setup
    config.rs      ~/.config/spwn/config.toml management
    client.rs      HTTP client wrapper (reqwest)
    commands/
      auth.rs      login, logout, whoami
      vm.rs        list, create, start, stop, delete, status
      snapshot.rs  list, take, restore, delete
      ssh.rs       spwn ssh <vm> (shell into VM)
      logs.rs      spwn logs <vm> (event log)
      config.rs    spwn config set/get
```

### commands

```
spwn login                          interactive email/password → stores session cookie
spwn logout                         clear stored session
spwn whoami                         show current account + quota usage

spwn vm list                        table: name, subdomain, status, vcores, mem
spwn vm create <name>               create a VM (--vcores, --memory, --image)
spwn vm start <name|id>             start a stopped VM
spwn vm stop <name|id>              stop a running VM
spwn vm delete <name|id>            delete a VM (--force to skip confirmation)
spwn vm status <name|id>            detailed status + events
spwn vm rename <name|id> <new>      rename a VM

spwn snapshot list <vm>             list snapshots for a VM
spwn snapshot take <vm>             take a snapshot (--label)
spwn snapshot restore <vm> <snap>   restore from snapshot
spwn snapshot delete <vm> <snap>    delete a snapshot

spwn ssh <vm>                       open SSH session to VM (via SSH gateway)
spwn ssh <vm> -- <command>          run a command and exit
spwn tunnel <vm> <local>:<remote>   forward local port to VM port
spwn logs <vm>                      stream VM events

spwn config set <key> <value>       set config (api-url, default-image, etc.)
spwn config get <key>               get config value
```

### auth flow

`spwn login` posts to `/auth/login`, receives the session cookie, stores it in `~/.config/spwn/credentials`. all subsequent requests include the cookie. `spwn logout` clears it.

### output

- default: human-readable tables (comfy-table or tabled)
- `--json` flag on every command for scripting
- `--quiet` flag suppresses non-essential output
- colored status indicators (green=running, yellow=starting, red=error, dim=stopped)

### control-plane API changes

minimal — the CLI is just a consumer of the existing API. a few additions:

- `GET /api/vms?name=<name>` — lookup by name (CLI uses names, not IDs)
- `GET /api/vms/:id/events` — paginated event log
- `PATCH /api/vms/:id` — rename, change exposed port

### deliverables

- [ ] clap command tree with all subcommands
- [ ] config file management (~/.config/spwn/)
- [ ] reqwest client with cookie jar persistence
- [ ] human-readable table output for all list commands
- [ ] `--json` flag on every command
- [ ] `spwn login` / `spwn logout` / `spwn whoami`
- [ ] full VM lifecycle commands
- [ ] snapshot commands
- [ ] `spwn ssh` stub (prints instructions until phase 8 SSH gateway exists)
- [ ] `spwn tunnel` stub
- [ ] colored output with status indicators
- [ ] shell completions (clap_complete for bash/zsh/fish)
- [ ] man page generation (clap_mangen)

---

## phase 8: SSH gateway (Go sidecar)

**goal:** users can `ssh spwn.run` and get an interactive TUI dashboard, or `ssh <vm-name>@spwn.run` to drop directly into a VM shell. TCP port forwarding works over SSH. the gateway is a Go binary using the charm stack.

### why Go / charm

the charmbracelet ecosystem (wish, bubbletea, lip gloss, bubbles) is the best thing that exists for building SSH-based TUI applications. wish gives you an SSH server where each connection gets a bubbletea program — meaning every user who connects gets a full interactive terminal UI with mouse support, styled output, and real-time updates. trying to replicate this in rust would mean building half of bubbletea from scratch.

the gateway is a sidecar: a separate binary with its own `go.mod`, deployed alongside the rust binaries. it talks to the rest of the platform exclusively via gRPC, so the language boundary is clean.

### project structure: `services/ssh-gateway/`

```
services/ssh-gateway/
  go.mod
  go.sum
  main.go                entry point, config loading, server start
  cmd/
    root.go              CLI flags (listen addr, gRPC endpoints, host key path)
  server/
    server.go            wish SSH server setup, middleware chain
    auth.go              password + public key auth via gRPC to control plane
    router.go            username parsing → VM lookup → session type dispatch
  tui/
    app.go               root bubbletea model (dispatches to sub-views)
    dashboard.go         VM list with status, start/stop actions
    vm_detail.go         single VM view: status, events, actions
    shell.go             raw terminal mode: relay I/O to VM via gRPC stream
    tunnel_status.go     active tunnel display
    styles.go            lip gloss style definitions
    keys.go              key bindings
  grpc/
    client.go            gRPC client to control plane + host agents
    proto/               symlink or copy of agent-proto .proto files
  tunnel/
    local.go             SSH -L forwarding handler
    remote.go            SSH -R forwarding handler
  config/
    config.go            env var / flag parsing
```

### gRPC integration

the gateway is a gRPC **client** to both the control plane and host agents.

**new proto: `proto/gateway.proto`** (added to `crates/agent-proto/proto/`):

```protobuf
syntax = "proto3";
package gateway;

service GatewayAuth {
  rpc VerifyPassword(VerifyPasswordRequest)   returns (VerifyPasswordResponse);
  rpc VerifyPublicKey(VerifyPublicKeyRequest) returns (VerifyPublicKeyResponse);
  rpc LookupVm(LookupVmRequest)             returns (LookupVmResponse);
}

message VerifyPasswordRequest {
  string username = 1;   // vm name or account email
  string password = 2;
}
message VerifyPasswordResponse {
  bool ok = 1;
  string account_id = 2;
  string error = 3;
}

message VerifyPublicKeyRequest {
  string public_key = 1;   // authorized_keys format
}
message VerifyPublicKeyResponse {
  bool ok = 1;
  string account_id = 2;
  string error = 3;
}

message LookupVmRequest {
  string account_id = 1;
  string vm_name = 2;     // name or subdomain
}
message LookupVmResponse {
  string vm_id = 1;
  string host_agent_addr = 2;
  string vm_ip = 3;
  string status = 4;
  int32 exposed_port = 5;
}
```

the control plane implements `GatewayAuth` as a gRPC service (alongside the existing agent registration service). the gateway calls it on every SSH auth attempt and VM lookup.

for shell relay, the gateway uses the existing `StreamConsole` RPC on the host agent (defined in `agent.proto` but currently stubbed). the host agent implements it by opening a connection to the VM's internal SSH server (or vsock in the future) and streaming bytes bidirectionally.

### authentication

two methods:

1. **password auth** — username is `<vm-name>` or just the account email. password is the user's spwn password. gateway calls `GatewayAuth.VerifyPassword` via gRPC.
2. **public key auth** — gateway calls `GatewayAuth.VerifyPublicKey` with the offered key. control plane checks against `ssh_keys` table.

### connection flow

```
user: ssh spwn.run
  → wish accepts connection
  → auth via gRPC (password or pubkey)
  → bubbletea dashboard TUI launches
  → user navigates VMs, picks one, hits enter
  → TUI switches to shell mode (raw I/O)
  → gateway opens StreamConsole gRPC stream to host agent
  → bidirectional byte relay until disconnect

user: ssh myvm@spwn.run
  → wish accepts connection
  → auth via gRPC
  → gateway calls LookupVm("myvm")
  → skips dashboard, goes directly to shell relay
  → StreamConsole gRPC stream to host agent
```

### the TUI

the dashboard bubbletea model shows:

```
┌─────────────────────────────────────────────┐
│  spwn                          nat@spwn.run │
│─────────────────────────────────────────────│
│                                             │
│  VMs                                        │
│                                             │
│  ● vivid-moon-be33    running   2c / 2gb    │
│  ○ quick-fox-a1b2     stopped   1c / 512mb  │
│  ● dark-star-ff01     running   4c / 4gb    │
│                                             │
│  [enter] connect  [s] start/stop  [n] new   │
│  [d] delete  [i] info  [q] quit             │
│                                             │
│  quota: 7/8 vcores  6.5/12gb ram  3/5 vms   │
└─────────────────────────────────────────────┘
```

pressing enter on a VM switches to shell mode (full terminal takeover). pressing `~.` (ssh escape) or `ctrl-]` drops back to the dashboard.

### port forwarding

SSH's built-in `-L` and `-R` forwarding, handled by wish's channel request handlers:

```bash
# forward local 8080 to VM's port 3000
ssh -L 8080:localhost:3000 myvm@spwn.run

# forward VM's port 9090 to your local machine
ssh -R 9090:localhost:9090 myvm@spwn.run
```

the gateway proxies TCP streams between the SSH channel and the VM's network (via the host agent). no caddy involvement.

### `spwn ssh` integration

`spwn ssh <vm>` invokes the system's `ssh` binary:

```bash
ssh -o "HostKeyAlias=spwn" -p 2222 <vm-name>@spwn.run
```

`spwn ssh` with no args opens the dashboard:

```bash
ssh -o "HostKeyAlias=spwn" -p 2222 spwn.run
```

### control-plane changes

- implement `GatewayAuth` gRPC service (VerifyPassword, VerifyPublicKey, LookupVm)
- SSH public key storage in `ssh_keys` table
- `POST /api/account/keys` / `GET /api/account/keys` / `DELETE /api/account/keys/:id` (HTTP API for frontend/CLI)

### host-agent changes

- implement `StreamConsole` RPC: opens SSH connection (or vsock) to the VM, relays bytes bidirectionally over gRPC stream
- the agent connects to the VM's internal SSH using a platform keypair (baked into rootfs, private key held by agent)

### db changes

```sql
CREATE TABLE ssh_keys (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    public_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    created_at BIGINT NOT NULL
);
```

### CLI additions (from phase 7)

```
spwn keys list                     list registered SSH public keys
spwn keys add <name> <path>        register a public key from file
spwn keys add <name> -             register from stdin
spwn keys remove <name|id>         remove a key
```

### VM-side requirements

VMs need an SSH server running inside them (dropbear or openssh-server, pre-installed in rootfs). the host agent holds a platform SSH keypair and the public key is baked into the rootfs's `authorized_keys`. the agent authenticates to the VM's sshd using this keypair when relaying console sessions.

future: replace VM-internal SSH with a vsock-based agent for lower overhead and no need for sshd in every image.

### env vars (ssh-gateway)

| var                  | default                      | description                   |
| -------------------- | ---------------------------- | ----------------------------- |
| `SSH_LISTEN_ADDR`    | `0.0.0.0:2222`               | SSH listen address            |
| `CONTROL_PLANE_GRPC` | `localhost:5000`             | gRPC address of control plane |
| `HOST_KEY_PATH`      | `/var/lib/spwn/ssh_host_key` | persistent ED25519 host key   |
| `DOMAIN`             | `spwn.run`                   | displayed in TUI header       |

### build & run

```bash
# build
cd services/ssh-gateway && go build -o ../../target/spwn-ssh-gateway .

# run (alongside control plane + agents)
SSH_LISTEN_ADDR=0.0.0.0:2222 \
CONTROL_PLANE_GRPC=localhost:5000 \
HOST_KEY_PATH=/var/lib/spwn/ssh_host_key \
./target/spwn-ssh-gateway
```

add to justfile:

```
ssh-gateway:
    cd services/ssh-gateway && go build -o ../../target/spwn-ssh-gateway .
    ./target/spwn-ssh-gateway
```

### deliverables

- [ ] Go module at `services/ssh-gateway/` with wish + bubbletea + lip gloss
- [ ] `gateway.proto` added to agent-proto, Go code generated
- [ ] control plane implements `GatewayAuth` gRPC service
- [ ] host agent implements `StreamConsole` RPC (SSH relay to VM)
- [ ] wish SSH server with password + pubkey auth
- [ ] bubbletea dashboard TUI (VM list, status, actions)
- [ ] VM detail view (events, info)
- [ ] shell relay mode (raw I/O via gRPC StreamConsole)
- [ ] escape sequence to return to dashboard from shell
- [ ] local port forwarding (-L)
- [ ] remote port forwarding (-R)
- [ ] SCP/SFTP passthrough
- [ ] host key persistence (ED25519, generate on first run)
- [ ] `spwn ssh` and `spwn ssh <vm>` working end-to-end
- [ ] `spwn tunnel` working end-to-end
- [ ] `spwn keys` commands (CLI + API)
- [ ] lip gloss styling with theme support
- [ ] rate limiting on auth attempts
- [ ] connection logging / audit trail
- [ ] justfile recipe for build + run

---

## phase 9: vm cloning

**goal:** users can clone VMs instantly, with copy-on-write semantics so clones share base image data and only store diffs.

### how it works

the storage model uses a two-layer approach:

**base layer (squashfs):** read-only, compressed, immutable. one per base image (alpine, ubuntu-minimal, debian-slim). shared across all VMs using that image. because squashfs is compressed, base images are significantly smaller on disk than their ext4 equivalents (~100-300MB vs 1-2GB).

**disk layer (ext4, per-VM):** a sparse ext4 file that stores the VM's writes. on a fresh VM this file is nearly empty — it only grows as the guest writes data. this is what makes "instant" creation possible: you're not copying a full disk image, you're creating an empty sparse file.

at VM boot, the two layers are combined via **overlayfs** on the host before being presented to firecracker as a single block device:

```bash
# per-VM mount (done by vm-manager before firecracker start)
mkdir -p /var/lib/vms/{vm_id}/{merged,upper,work}
mount -t overlay overlay \
  -o lowerdir=/var/lib/images/ubuntu-minimal.squashfs,upperdir=/var/lib/vms/{vm_id}/upper,workdir=/var/lib/vms/{vm_id}/work \
  /var/lib/vms/{vm_id}/merged
```

firecracker then gets `/var/lib/vms/{vm_id}/merged` as its rootfs drive.

**cloning a VM** means:

1. pause the source VM (or snapshot it)
2. create a new VM record in the DB with a new ID, IP, subdomain
3. copy the source VM's upper layer → becomes the clone's upper layer (same base image, no need to touch squashfs)
4. if cloning from a snapshot: copy the snapshot's memory file too, restore into a new firecracker process
5. if cloning from a paused VM: take a snapshot first, then clone from that

the upper layer copy is the only I/O cost, and it's proportional to how much the source VM has diverged from the base image — a fresh VM with minimal changes clones in seconds.

**note on "copy-on-write":** this isn't CoW at the block level (like btrfs reflinks or device-mapper thin snapshots would give you). overlayfs operates at the file level — when the guest modifies a file, the whole file gets copied to the upper layer. for a hobbyist platform this is fine; block-level CoW would only matter if users are doing heavy random writes to large files. if you want true block-level CoW later, you'd swap the upper layer to a btrfs subvolume and use `cp --reflink` for clones, keeping squashfs as the base.

### storage layout

```
/var/lib/images/
├── alpine.squashfs            # ~50MB, shared by all alpine VMs
├── ubuntu-minimal.squashfs    # ~200MB, shared by all ubuntu VMs
└── debian-slim.squashfs       # ~150MB, shared by all debian VMs

/var/lib/vms/
├── {vm_id_1}/
│   ├── upper/                 # ext4 sparse file, VM's writable layer
│   ├── work/                  # overlayfs workdir (required, don't touch)
│   └── merged/                # overlayfs mount point → firecracker rootfs
├── {vm_id_2}/
│   └── ...
```

### data model changes

```sql
-- add to vms table
ALTER TABLE vms ADD COLUMN base_image TEXT NOT NULL DEFAULT 'ubuntu-minimal';
ALTER TABLE vms ADD COLUMN cloned_from TEXT REFERENCES vms(id);
ALTER TABLE vms ADD COLUMN upper_layer_path TEXT NOT NULL;

-- track disk usage (upper layer only, since base is shared)
ALTER TABLE vms ADD COLUMN disk_usage_mb INTEGER NOT NULL DEFAULT 0;
```

### api changes

```
POST   /api/vms/:id/clone     -- clone a VM
```

request body:

```json
{
  "name": "my-clone",
  "subdomain": "my-clone-abc", // optional, auto-generate if omitted
  "include_memory": true // if true, clone from snapshot (preserves running state)
}
```

### integration

**vm-manager:**

- new `clone_vm` method that orchestrates: pause/snapshot source → create DB record → copy upper layer → (optionally) restore snapshot into new process → configure TAP + IP → register caddy route
- overlayfs mount/unmount added to VM start/stop lifecycle (mount before firecracker start, unmount after firecracker exit)
- update reconciliation loop to handle orphaned overlayfs mounts

**networking:**

- clone gets a new IP from the CIDR pool, new TAP device, new caddy route — same as any new VM
- no networking state is shared between source and clone

**quota:**

- clone counts against the account's resource limits (vcores, memory) like any other VM
- upper layer disk usage counts toward the account's storage quota
- cloning a VM with `include_memory: true` also creates a snapshot, which counts toward the snapshot limit

**base image management:**

- squashfs images are built offline and placed in `/var/lib/images/`
- add a script to `scripts/` that takes an ext4 rootfs and converts it: `mksquashfs rootfs.ext4 image.squashfs -comp zstd`
- images are versioned (e.g. `ubuntu-minimal-20240115.squashfs`) with a symlink for the current version
- updating a base image doesn't affect running VMs (they have their own upper layer); new VMs get the latest

### deliverables

- [ ] overlayfs mount/unmount in vm-manager start/stop lifecycle
- [ ] squashfs base image build script (`scripts/build-base-image.sh`)
- [ ] 3 curated base images built and tested (alpine, ubuntu-minimal, debian-slim)
- [ ] `POST /api/vms/:id/clone` endpoint with quota enforcement
- [ ] upper layer copy logic (with progress tracking for large clones)
- [ ] `include_memory` clone path (snapshot → restore into new process)
- [ ] DB migration for new columns (`base_image`, `cloned_from`, `upper_layer_path`, `disk_usage_mb`)
- [ ] reconciliation loop updated to clean up orphaned overlayfs mounts
- [ ] disk usage tracking: periodic scan of upper layer sizes, update `disk_usage_mb`
- [ ] frontend: clone button on VM detail page, clone modal with name/subdomain/memory options

---

## phase 10: proper multi-node

**goal:** spwn runs across multiple physical hosts reliably. VMs are scheduled intelligently. host failures are detected and handled.

### what "proper" means

right now multi-node technically works (agent registers, control plane dispatches), but:

- scheduling is naive (highest free memory)
- no host failure detection beyond heartbeat timeout
- no VM migration
- no capacity planning
- no agent version tracking

### scheduling improvements

**placement strategy:**

```rust
enum PlacementStrategy {
    BestFit,          // minimize wasted resources (pack tightly)
    SpreadAcross,     // spread VMs across hosts (resilience)
    Pinned(HostId),   // user requested a specific host
}
```

default: `SpreadAcross` for free-tier users, `BestFit` for density.

**anti-affinity:** a user's VMs should spread across hosts when possible so a single host failure doesn't take out all their VMs.

**pg advisory locks for scheduling:** prevent two concurrent create requests from double-booking the same capacity:

```rust
async fn schedule_vm(pool: &PgPool, vm: &VmRow) -> Result<HostId> {
    // acquire advisory lock on scheduling
    // find eligible hosts (alive, has capacity)
    // pick based on strategy
    // update vm.host_id
    // release lock
}
```

### host health

- heartbeat timeout: 30s → mark host `degraded`, 90s → mark `unreachable`
- `degraded` hosts don't receive new VMs
- `unreachable` hosts: their VMs are marked `unknown` — don't auto-restart (could be a network partition, not a real failure)
- admin endpoint to manually drain a host (`POST /admin/hosts/:id/drain`)

### host drain

for maintenance:

1. stop scheduling new VMs to the host
2. snapshot all running VMs
3. stop all VMs on the host
4. mark host `draining`
5. (future) restore VMs on other hosts

### db changes

```sql
ALTER TABLE hosts ADD COLUMN status TEXT NOT NULL DEFAULT 'active';
-- active | degraded | unreachable | draining | offline

ALTER TABLE hosts ADD COLUMN agent_version TEXT;
ALTER TABLE hosts ADD COLUMN labels JSONB DEFAULT '{}';
```

### deliverables

- [ ] placement strategy enum + configurable default
- [ ] anti-affinity scoring in scheduler
- [ ] pg advisory lock on scheduling
- [ ] host health state machine (active → degraded → unreachable)
- [ ] degraded hosts excluded from scheduling
- [ ] admin drain endpoint
- [ ] host labels (for future zone/region awareness)
- [ ] agent version tracking
- [ ] dashboard: host list with status, capacity, VM count

---

## phase 11: proper testing

**goal:** comprehensive test suite that catches regressions. cargo-nextest for parallel execution. CI that actually means something.

### test infrastructure

**cargo-nextest:** faster parallel test execution, better output, test partitioning for CI.

```toml
# .config/nextest.toml
[profile.default]
retries = 0
slow-timeout = { period = "60s", terminate-after = 2 }
fail-fast = false

[profile.ci]
retries = 2
fail-fast = true
```

### test categories

**unit tests** (in each crate):

- `crates/common` — type conversions, validation
- `crates/networking` — IP allocation logic, CIDR math (mocked iptables/TAP calls)
- `crates/router-sync` — caddy request serialization (mock HTTP)
- `crates/db` — query logic against testcontainers postgres
- `crates/auth` — password hashing, session validation
- `crates/api` — handler logic with mock VmOps
- `crates/cli` — argument parsing, output formatting

**integration tests** (in `tests/` or per-crate `tests/`):

- `crates/db/tests/` — full migration + CRUD cycles against real postgres (testcontainers)
- `crates/auth/tests/` — signup/login/logout flow against real postgres
- `crates/api/tests/` — full API flow with mock VmOps (already exists)
- `crates/control-plane/tests/` — gRPC client ↔ mock agent
- `crates/ssh-gateway/tests/` — SSH auth + session establishment (russh test client)

**end-to-end tests** (require KVM, run separately):

- boot a VM, verify networking, stop it, delete it
- snapshot + restore round-trip
- SSH gateway → VM shell session
- port forwarding through gateway
- multi-agent scheduling

### test utilities crate

`crates/test-utils/` — shared helpers:

```rust
pub async fn test_pool() -> PgPool           // testcontainers postgres
pub async fn test_account(pool: &PgPool) -> AccountRow  // create a test account
pub async fn test_vm(pool: &PgPool, account_id: &str) -> VmRow  // create a test VM
pub fn mock_vm_ops() -> MockVmOps            // mock for API tests
```

### CI pipeline

```yaml
# run on every PR
test-unit: cargo nextest run --profile ci --partition count:1/3
  cargo nextest run --profile ci --partition count:2/3
  cargo nextest run --profile ci --partition count:3/3

test-integration:
  # needs podman socket
  cargo nextest run --profile ci -E 'test(integration)'

lint: cargo clippy --all-targets -- -D warnings
  cargo fmt --check

# run nightly or on main merges (needs KVM host)
test-e2e: cargo nextest run --profile ci -E 'test(e2e)'
```

### what to test that isn't tested today

- quota enforcement race condition (concurrent starts)
- reconciler behavior when firecracker process dies
- caddy route rebuild correctness
- session expiry
- snapshot limit enforcement
- overlay provisioning edge cases (disk full, permission denied)
- gRPC reconnection after agent restart
- heartbeat timeout → host degraded transition

### deliverables

- [ ] cargo-nextest config
- [ ] test-utils crate with shared helpers
- [ ] unit tests for every crate (minimum: happy path + one error case per public function)
- [ ] integration tests for db, auth, api, control-plane, ssh-gateway
- [ ] e2e test harness (boots real VMs, needs KVM)
- [ ] CI pipeline config (github actions or similar)
- [ ] test coverage tracking (cargo-llvm-cov)
- [ ] property-based tests for IP allocation (proptest)

---

## phase 12: playful features

these are the things that make spwn feel like a playground rather than AWS.

### VM templates

pre-configured VM images with software already installed:

```
spwn vm create --template node       # node 22 + pnpm
spwn vm create --template python     # python 3.12 + pip + venv
spwn vm create --template go         # go 1.23
spwn vm create --template rust       # rustup + stable toolchain
spwn vm create --template postgres   # postgres 16 running on boot
spwn vm create --template blank      # just alpine, nothing extra
```

templates are squashfs images stored on each host. the control plane serves a template catalog.

### dotfiles sync

users register a dotfiles repo. on first SSH into a new VM, the gateway (or an init script) clones and runs the install script.

```
spwn config set dotfiles https://github.com/user/dotfiles
```

### VM nicknames + MOTD

VMs can have a custom MOTD that shows on SSH login. set via CLI:

```
spwn vm motd <vm> "welcome to the danger zone"
```

### shared VMs (collaborative)

invite another spwn user to SSH into your VM:

```
spwn vm share <vm> <email>          # grant access
spwn vm unshare <vm> <email>        # revoke
spwn vm shares <vm>                 # list who has access
```

shared users authenticate via the SSH gateway and get routed to the VM. the VM owner's quota is used.

### webhook notifications

get notified when stuff happens:

```
spwn hooks add <vm> <url>           # POST to URL on start/stop/crash
spwn hooks list <vm>
spwn hooks remove <vm> <hook-id>
```

### VM auto-stop

VMs that are idle (no SSH sessions, no HTTP traffic) for N hours auto-stop to save resources:

```
spwn vm set <vm> auto-stop 4h       # stop after 4 hours idle
spwn vm set <vm> auto-stop off      # disable
```

tracked by the host agent — monitors network activity and SSH sessions.

### deliverables

- [ ] template catalog API + CLI
- [ ] 5 base templates (node, python, go, rust, blank)
- [ ] template build pipeline (scripts to produce squashfs images)
- [ ] dotfiles config + sync-on-first-SSH
- [ ] VM MOTD support
- [ ] shared VM access (invite by email)
- [ ] webhook notifications for VM events
- [ ] auto-stop on idle

---

## phase 13: billing

billing is last because nothing above requires it. the platform should feel complete and fun before we ask for money.

### lemonsqueezy integration

same plan as v2 but simplified:

```
free tier:    1 VM, 2 vcores, 2gb ram, no snapshots
pro tier:     5 VMs, 8 vcores, 12gb ram, 2 snapshots/VM     $12/mo
hacker tier:  10 VMs, 16 vcores, 24gb ram, 5 snapshots/VM   $24/mo
```

### what to build

- lemonsqueezy webhook handler (idempotent, signature verified)
- `processed_webhooks` table for deduplication
- account tier enforcement in quota checks
- `spwn billing` CLI commands (status, portal link)
- frontend billing page with plan comparison + upgrade flow
- trial period: 7 days of pro tier on signup, no credit card required

### deliverables

- [ ] lemonsqueezy product setup (3 tiers)
- [ ] webhook handler with signature verification
- [ ] account tier → quota mapping
- [ ] `spwn billing status` / `spwn billing upgrade`
- [ ] frontend billing page
- [ ] trial period logic
- [ ] grace period on payment failure (3 days before downgrade)

---

## phase 14: hardening

### observability

- structured logging everywhere (tracing crate, already partially in place)
- prometheus metrics endpoint on control plane + agents
- key metrics: VM boot time, API latency, active VMs, host utilization, SSH sessions
- optional grafana dashboard (ship a JSON template)

### security

- rate limiting on all auth endpoints (tower-governor)
- rate limiting on VM create (prevent quota abuse via rapid create/delete)
- SSH brute-force protection (fail2ban-style in the gateway)
- audit log for all destructive actions
- TLS on internal gRPC (agent ↔ control plane)
- secret rotation for session signing keys

### reliability

- graceful shutdown for all binaries (drain connections, finish in-flight ops)
- backup strategy for postgres (pg_dump cron + offsite)
- snapshot storage backup
- runbook documentation for common failure modes

### deliverables

- [ ] prometheus metrics on all binaries
- [ ] grafana dashboard template
- [ ] rate limiting on auth + VM create
- [ ] SSH brute-force protection
- [ ] audit log table + API
- [ ] internal gRPC TLS
- [ ] graceful shutdown handlers
- [ ] backup scripts + documentation
- [ ] operational runbook

---

## phase summary

| phase | what                              | status      |
| ----- | --------------------------------- | ----------- |
| 1     | firecracker spike                 | done        |
| 2     | caddy routing                     | done        |
| 3     | VM lifecycle API + reconciliation | done        |
| 4     | snapshot/restore + overlayfs      | done        |
| 4b    | control plane + host agent split  | done        |
| 5     | auth + accounts                   | done        |
| 6     | frontend                          | done        |
| 7     | CLI                               | not started |
| 8     | SSH gateway (Go/charm)            | not started |
| 9     | VM cloning                        | not started |
| 10    | proper multi-node                 | not started |
| 11    | proper testing                    | not started |
| 12    | playful features                  | not started |
| 13    | billing                           | not started |
| 14    | hardening                         | not started |

phases 7 and 11 can run in parallel — CLI development and test infrastructure don't block each other. phase 8 depends on 7 (CLI is the primary SSH client interface). phases 9 and 10 can run in parallel after 8. phase 9 (VM cloning) is relatively self-contained and could also start earlier. 12 is a grab bag that can be interleaved with anything. 13 and 14 come last.

---

## things to defer (still)

- custom domains (cert management complexity)
- VM migration between hosts (snapshot + restore on new host works as a manual process)
- teams / organizations
- VM marketplace (user-published templates)
- GPU passthrough
- IPv6 per VM
- bring-your-own-rootfs (templates cover 90% of use cases)
- web-based terminal (SSH gateway is the primary interface)

---

## decisions carried forward from v2

- **networking:** 172.16.0.0/16, slot-based TAP naming (fc-tap-{slot}), static IPs via kernel boot args
- **storage:** shared squashfs base + per-VM sparse ext4 overlay
- **routing:** caddy with wildcard TLS via DNS-01, dynamic config via admin API
- **auth:** argon2 + session cookies, invite-only signup
- **quota:** serializable transaction, per-account limits in accounts table
- **reconciliation:** agent-side, runs on startup + periodically, rebuilds caddy routes from DB

---

## open questions

- **Go proto generation:** generate Go code from the same `.proto` files in `crates/agent-proto/proto/`, or maintain a copy in `services/ssh-gateway/proto/`? symlink is cleanest but can be fragile. a `just proto` recipe that generates both rust and Go from the same source is probably the move.
- **VM-internal SSH vs vsock:** baking SSH into the rootfs is simple but means every template needs sshd. vsock agent is cleaner but more work. start with SSH, evaluate vsock later.
- **tunnel subdomain format:** `<vm>-<port>.spwn.run` or `<random>.tunnels.spwn.run`? the former is predictable, the latter avoids leaking VM names.
- **free tier limits:** 1 VM with 2 vcores and 2gb is generous enough to be useful but constrained enough to convert. need to test this assumption.
- **host key persistence:** generate ED25519 key on first run, persist to `HOST_KEY_PATH`. for multi-instance gateway (load balanced), all instances need the same key — mount from a shared secret or store in postgres.
- **bubbletea theme sync:** should the SSH TUI respect the user's theme preference from the web frontend? could fetch it via gRPC on connect.
