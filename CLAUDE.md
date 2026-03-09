# spwn

firecracker-based hobbyist VM platform. users get a fixed resource pool (8 vcores, 12gb ram, 5 vms), persistent microVMs, and wildcard subdomain routing via caddy.

full architecture plan: `.claude/plan-v2.md`
phase plans: `.claude/phase-{1..8}.md`

---

## crate layout

```
crates/common        shared types (VmId, etc.)
crates/networking    TAP devices, iptables, IP allocation
crates/db            sqlx/postgres queries + migrations
crates/auth          password hashing (argon2), session extractor, auth routes
crates/api           axum router + VmOps trait (shared between cp and tests)
crates/router-sync   caddy admin API client
crates/agent-proto   .proto + tonic generated code (shared by agent + control-plane)
crates/host-agent    firecracker process management + gRPC server (needs root)
crates/control-plane external HTTP API + scheduler + gRPC client to agents
frontend/            react + tanstack router/query (vite, pnpm)
```

---

## running things

configure via `.env` (loaded automatically by `just`):

```bash
just cp         # build + run control-plane (HTTP :3019, gRPC :5000)
just agent      # build + run host-agent (gRPC :4000, prompts for sudo)
just frontend   # vite dev server (:5173, proxies /auth + /api to :3019)
just pg         # start postgres via podman compose
just pg-reset   # wipe + restart postgres
just test       # run db + auth integration tests (needs podman socket)
just check      # cargo check across workspace
```

**host-agent needs root** — it manages TAP devices and firecracker processes. `just agent` handles `sudo -E` automatically.

**frontend dev**: `just frontend` proxies API requests to the control-plane. for production, build with `just frontend-build` and point `FRONTEND_PATH` at `frontend/dist`.

---

## key env vars

| var | used by | default |
|---|---|---|
| `DATABASE_URL` | cp, agent | postgres://postgres:spwn@localhost/spwn |
| `LISTEN_ADDR` | cp | 0.0.0.0:3019 |
| `GRPC_LISTEN_ADDR` | cp | 0.0.0.0:5000 |
| `INVITE_CODE` | cp | *(required)* |
| `FRONTEND_PATH` | cp | frontend/dist |
| `CADDY_URL` | cp | http://localhost:2019 |
| `STATIC_FILES_PATH` | cp | /var/lib/spwn/static |
| `AGENT_LISTEN_ADDR` | agent | 0.0.0.0:4000 |
| `AGENT_PUBLIC_ADDR` | agent | http://localhost:4000 |
| `CONTROL_PLANE_URL` | agent | http://localhost:5000 |
| `KERNEL_PATH` | agent | /tmp/vmlinux |
| `ROOTFS_PATH` | agent | /tmp/rootfs.sqfs |
| `FIRECRACKER_BIN` | agent | ~/.local/bin/firecracker |

---

## testing

integration tests use testcontainers + podman. the podman socket must be running:

```bash
systemctl --user start podman.socket
just test
```

tests live in:
- `crates/db/tests/integration.rs` — account/session CRUD, quota enforcement
- `crates/auth/tests/integration.rs` — signup/login/logout/me routes

---

## gotchas

- **protoc required** — `sudo pacman -S protobuf` (or distro equivalent) for agent-proto build
- **TAP device names ≤15 chars** — use slot number not VM UUID (`fc-tap-{slot}`)
- **TAP devices survive crashes** — reconciler resets stuck `starting`/`stopping` VMs on startup
- **`sudo -E` for agent** — cargo isn't on sudo's PATH; build first, then run the binary
- **caddy dynamic config is ephemeral** — rebuild all routes from DB on startup; never rely on caddy persisting dynamic state
- **caddy admin API must bind to 127.0.0.1:2019** — VMs must not reach it (iptables DROP rule)
- **quota check uses SERIALIZABLE transaction** — prevents race on concurrent start requests; caller retries once on serialization failure
- **migrations embed at compile time** — `crates/db/build.rs` triggers recompile when `migrations/` changes; still need to `touch` or rebuild after adding new migration files if sqlx doesn't pick them up
- **Rust 2024 edition**: `gen` is reserved — use `gen_range` etc.
- **`thread_rng()` is not `Send`** — drop before any `.await`
- **fish shell** — use zsh or inline env for sudo commands

---

## networking scheme

- CIDR: `172.16.0.0/16`
- VM slot N: host TAP `172.16.N.1/30`, guest `172.16.N.2/30`
- guest IP set via kernel boot args (no DHCP): `ip=<guest>::<host>:255.255.255.252::eth0:off`
- external iface auto-detected via `ip route show default` if `EXTERNAL_IFACE` not set

---

## phase status

- phase 1 (firecracker spike): **done**
- phase 2 (caddy routing): **done**
- phase 3 (vm lifecycle API + reconciliation): **done**
- phase 4 (snapshot/restore + overlayfs): **done**
- phase 4b (control plane + host agent split): **done**
- phase 5 (auth + accounts): **done**
- phase 6 (frontend): **done**
- phase 7 (billing — lemonsqueezy): not started
- phase 8 (hardening): not started

---

## git workflow

use feature branches:

```bash
git checkout -b feature/descriptive-name
# branch prefixes: feature/, fix/, docs/, refactor/, test/
```

commit regularly. don't push directly to main.
