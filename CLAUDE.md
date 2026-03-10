# spwn

firecracker-based hobbyist VM platform. users get a fixed resource pool, persistent microVMs, and wildcard subdomain routing via caddy.

---

## package/crate layout

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
services/ssh-gateway Go sidecar (wish/bubbletea/lipgloss), SSH TUI + shell relay via gRPC
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

**note: host-agent needs root** — it manages TAP devices and firecracker processes. `just agent` handles `sudo -E` automatically.

**frontend dev**: `just frontend` proxies API requests to the control-plane. for production, build with `just frontend-build` and point `FRONTEND_PATH` at `frontend/dist`.

---

## key env vars

| var                         | used by   | default                                 |
| --------------------------- | --------- | --------------------------------------- |
| `DATABASE_URL`              | cp, agent | postgres://postgres:spwn@localhost/spwn |
| `LISTEN_ADDR`               | cp        | 0.0.0.0:3019                            |
| `GRPC_LISTEN_ADDR`          | cp        | 0.0.0.0:5000                            |
| `INVITE_CODE`               | cp        | _(required)_                            |
| `PUBLIC_URL`                | cp        | https://spwn.run                        |
| `FRONTEND_PATH`             | cp        | frontend/dist                           |
| `CADDY_URL`                 | cp        | http://localhost:2019                   |
| `STATIC_FILES_PATH`         | cp        | /var/lib/spwn/static                    |
| `AGENT_LISTEN_ADDR`         | agent     | 0.0.0.0:4000                            |
| `AGENT_PUBLIC_ADDR`         | agent     | http://localhost:4000                   |
| `CONTROL_PLANE_URL`         | agent     | http://localhost:5000                   |
| `KERNEL_PATH`               | agent     | /tmp/vmlinux                            |
| `ROOTFS_PATH`               | agent     | /tmp/rootfs.sqfs                        |
| `FIRECRACKER_BIN`           | agent     | ~/.local/bin/firecracker                |
| `JAILER_BIN`                | agent     | /usr/local/bin/jailer                   |
| `JAILER_UID`                | agent     | uid of `spwn-vm` user (auto-resolved)   |
| `JAILER_GID`                | agent     | gid of `spwn-vm` group (auto-resolved)  |
| `JAILER_CHROOT_BASE`        | agent     | /srv/jailer                             |
| `SSH_GATEWAY_ADDR`          | cp        | localhost:2222                          |
| `GATEWAY_SECRET`            | cp, gw    | _(required)_                            |
| `SSH_GATEWAY_LISTEN_ADDR`   | gw        | 0.0.0.0:2222                            |
| `SSH_GATEWAY_HOST_KEY_PATH` | gw        | /var/lib/spwn/gateway_host_key          |
| `CONTROL_PLANE_HTTP_URL`    | gw        | http://localhost:3019                   |
| `PLATFORM_KEY_PATH`         | agent     | /var/lib/spwn/platform_key              |

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
- **jailer requires a dedicated unprivileged user** — create `spwn-vm` with `useradd -r -s /sbin/nologin spwn-vm`, or set `JAILER_UID`/`JAILER_GID` explicitly. the agent resolves uid/gid from `/etc/passwd` and `/etc/group` at startup if the env vars are absent
- **jailer chroot layout** — each VM jail lives at `<JAILER_CHROOT_BASE>/firecracker/<vm_id>/root/`. snapshots are written inside the jail root and their host-side paths (e.g. `/srv/jailer/firecracker/<vm_id>/root/<snap>.snap`) are what gets stored in the DB
- **PID tracking with new PID namespace** — `exec_in_new_pid_ns` is enabled; the jailer writes `<jail_root>/firecracker.pid` with the outer host PID. reconciler falls back to reading `/sys/fs/cgroup/firecracker/<vm_id>/cgroup.procs` if the pid file isn't present
- **TAP device names ≤15 chars** — use slot number not VM UUID (`fc-tap-{slot}`)
- **TAP devices survive crashes** — reconciler resets stuck `starting`/`stopping` VMs on startup
- **`sudo -E` for agent** — cargo isn't on sudo's PATH; build first, then run the binary
- **caddy dynamic config is ephemeral** — rebuild all routes from DB on startup; never rely on caddy persisting dynamic state
- **caddy admin API must bind to 127.0.0.1:2019** — VMs must not reach it (iptables DROP rule)
- **quota check uses SERIALIZABLE transaction** — prevents race on concurrent start requests; caller retries once on serialization failure
- **migrations embed at compile time** — `crates/db/build.rs` triggers recompile when `migrations/` changes; still need to `touch` or rebuild after adding new migration files if sqlx doesn't pick them up
- **platform SSH key bootstrap** — the platform key is generated lazily on the first `StreamConsole` call, not at agent startup. if `/var/lib/spwn/platform_key` already exists it loads silently. get the public key with `ssh-keygen -y -f /var/lib/spwn/platform_key` and add it to the rootfs, or use `sudo scripts/spwn inject-platform-key <image>` to rebuild the squashfs with it injected automatically
- **gateway TOFU known_hosts** — CLI stores gateway host key at `~/.config/spwn/known_hosts` on first connect; key mismatch hard-fails to prevent MITM
- **`GATEWAY_SECRET` must match** — control-plane and ssh-gateway must share the same value; gateway calls `/internal/gateway/auth/*` endpoints protected by `Bearer <GATEWAY_SECRET>`. running the gateway with `sudo` strips env vars — use `sudo -E` and make sure the calling shell has already sourced `.env`, or pass vars explicitly: `sudo GATEWAY_SECRET=x CONTROL_PLANE_HTTP_URL=y ./spwn-ssh-gateway`
- **gateway uses VM ID as SSH username** — the CLI sends `vm.ID` (UUID) as the SSH username, not the name. the gateway looks up `/internal/gateway/vm?vm_id=<uuid>` which calls `db::get_vm` by ID. don't change this to name — names can contain spaces and other characters that break SSH usernames
- **pubkey auth context is ephemeral** — `gliderlabs/ssh` calls the `PublicKeyHandler` twice (probe + real auth). `ctx.SetValue` in the handler does not survive to the session handler. instead, re-resolve the account ID inside the session handler using `s.PublicKey()` and a second call to the auth endpoint
- **host agent address includes scheme** — `host_agent_addr` stored in the DB is a full URL (`http://localhost:4000`). strip the scheme before passing to `grpc.NewClient` which expects `host:port`
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
- phase 7 (CLI): **done**
- phase 8 (SSH gateway — Go/charm): **done**
- phase 9 (VM cloning): not started
- phase 10 (proper multi-node): not started
- phase 11 (proper testing + cargo-nextest): not started
- phase 12 (playful features — templates, dotfiles, sharing): not started
- phase 13 (billing, lemonsqueezy): not started
- phase 14 (hardening): not started

---

## git workflow

use feature branches:

```bash
git checkout -b natb/feature-descriptive-name
# branch prefixes: feature/, fix/, docs/, refactor/, test/
```

commit regularly. don't push directly to main.
