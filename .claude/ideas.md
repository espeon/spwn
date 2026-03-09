# spwn — feature ideas

raw braindump. none of this is planned or promised. just things that would be cool.

---

## terminal & CLI

- **spwn repl** — interactive mode with tab completion, fuzzy VM search, inline status updates
- **spwn watch** — live dashboard in the terminal (like `htop` for your VMs). CPU, memory, disk, network per VM. refresh every 2s
- **spwn cp <local> <vm>:<path>** — scp-style file copy without needing to set up SSH keys manually
- **spwn exec <vm> -- <cmd>** — run a one-shot command, stream stdout/stderr, exit with the command's exit code
- **spwn attach <vm>** — attach to the VM's serial console (not SSH — raw serial)
- **spwn port <vm>** — show all listening ports inside the VM (query via agent)
- **spwn top** — aggregate resource usage across all your VMs in one view
- **spwn diff <vm> <snapshot>** — show filesystem changes since a snapshot (overlayfs diff)
- **spwn history <vm>** — git-log-style timeline of VM events with relative timestamps
- **spwn init** — interactive project setup: pick a template, name your VM, configure ports, generate a `spwn.toml`
- **spwn.toml** — project-local config file. define VMs declaratively. `spwn up` reads it and creates/starts everything
- **shell prompt integration** — `spwn shell-prompt` outputs current VM context for PS1 integration
- **CLI plugins** — `spwn x <plugin>` runs community-contributed extensions from a plugin registry

---

## networking & connectivity

- **custom domains** — `spwn domain add <vm> myapp.com` with automatic cert provisioning via caddy
- **wildcard ports** — `<port>-<vm>.spwn.run` routes to any port without explicit configuration
- **TCP passthrough** — raw TCP forwarding for non-HTTP services (databases, game servers, etc.)
- **UDP forwarding** — for DNS servers, game servers, wireguard endpoints inside VMs
- **inter-VM networking** — private network between your VMs. `spwn net create mynet` + `spwn net join mynet <vm>`
- **IPv6 per VM** — each VM gets a public IPv6 address, no NAT needed
- **wireguard mesh** — auto-configured wireguard between your VMs across hosts. flat network, no overlay
- **DNS for VMs** — `<vm>.internal` resolves inside your private network
- **bandwidth monitoring** — per-VM traffic graphs, daily/monthly totals
- **egress controls** — allowlist/blocklist outbound destinations per VM (iptables rules managed via API)
- **mTLS between VMs** — automatic certificate issuance for inter-VM communication

---

## developer experience

- **git push deploy** — `git remote add spwn spwn://<vm>/app` → push triggers a build + restart inside the VM
- **live sync** — `spwn sync <local-dir> <vm>:<remote-dir>` watches for changes and rsyncs in real-time
- **VS Code remote SSH config** — `spwn vscode <vm>` generates/updates `~/.ssh/config` for one-click Remote-SSH
- **GitHub Codespaces-style URLs** — open a web IDE (code-server) running inside the VM at `ide-<vm>.spwn.run`
- **preview environments** — `spwn preview <branch>` spins up a VM from a git branch with a unique URL
- **environment variables** — `spwn env set <vm> KEY=value` injects env vars accessible inside the VM
- **secrets management** — encrypted at rest, injected at boot, never visible in API responses
- **build caching** — shared read-only layer with common build artifacts (node_modules, cargo registry, pip cache)
- **dev containers** — support devcontainer.json spec for automatic VM setup from a repo config
- **language detection** — auto-detect project language on `spwn init` and suggest the right template + ports

---

## snapshots & state

- **scheduled snapshots** — `spwn snapshot schedule <vm> daily 3am` with retention policy
- **snapshot labels + notes** — `spwn snapshot take <vm> --label "before migration" --note "about to upgrade postgres"`
- **snapshot diff viewer** — web UI showing changed files between two snapshots
- **snapshot sharing** — `spwn snapshot share <vm> <snap> <email>` lets another user restore your snapshot into their VM
- **snapshot export** — download a snapshot as a tarball for local backup
- **snapshot import** — upload a snapshot from another platform or local backup
- **VM cloning** — `spwn vm clone <vm> <new-name>` creates a new VM from the current state (snapshot + restore in one step)
- **checkpoint/rollback** — named save points. `spwn checkpoint <vm> before-yolo` → do risky thing → `spwn rollback <vm> before-yolo`
- **time travel debugging** — take a snapshot every N minutes, browse/restore any point in time
- **immutable VMs** — option to mark a VM as immutable: overlay resets on every reboot. persistent state only in mounted volumes

---

## collaboration

- **pair programming** — two users in the same SSH session (tmux-style shared terminal via the SSH gateway)
- **VM transfer** — `spwn vm transfer <vm> <email>` moves ownership to another user
- **organizations** — shared VM pool with role-based access (admin, member, viewer)
- **activity feed** — see what your collaborators are doing across shared VMs
- **shared snapshots library** — org-wide snapshot catalog. "here's a known-good database state for testing"
- **VM comments** — leave notes on a VM visible to all collaborators. "don't restart this, migration running"
- **live presence indicators** — see who's currently SSH'd into a shared VM

---

## monitoring & observability

- **VM metrics dashboard** — CPU, memory, disk I/O, network I/O graphs in the web UI
- **log streaming** — `spwn logs <vm>` streams syslog/journald from inside the VM
- **uptime monitoring** — built-in health checks. `spwn health add <vm> http://localhost:3000/health`
- **alerting** — get notified (webhook, email, slack) when a VM crashes, health check fails, or disk is 90% full
- **cost estimation** — show estimated monthly cost based on current resource usage
- **resource recommendations** — "this VM has used <1 vcore average over 30 days, consider downsizing"
- **boot time tracking** — measure and display VM cold boot and snapshot restore times
- **request logging** — optional HTTP request log for traffic through caddy routes (method, path, status, latency)
- **error rate tracking** — track 5xx responses through the proxy, alert on spikes

---

## images & templates

- **community templates** — user-published templates with ratings and install counts
- **template marketplace** — browse, search, one-click deploy
- **Dockerfile → template** — upload a Dockerfile, we build it into a squashfs image
- **nix flake → template** — `spwn vm create --nix github:user/repo#vm` builds from a nix flake
- **template versioning** — templates have versions. `spwn vm create --template node:22` vs `node:20`
- **golden images** — snapshot a perfectly-configured VM, publish it as a personal template
- **base image auto-updates** — security patches applied to base images weekly, opt-in auto-rebuild for VMs
- **minimal images** — stripped-down images for specific use cases (static site server: 20MB, API server: 50MB)
- **windows support** — just kidding. unless?

---

## automation & scripting

- **cloud-init** — support cloud-init user-data for automated VM provisioning
- **startup scripts** — `spwn vm set <vm> startup-script ./setup.sh` runs on every boot
- **webhooks on everything** — VM started, stopped, crashed, snapshot taken, disk 80% full, SSH session opened
- **API tokens** — long-lived tokens for CI/CD integration (separate from session cookies)
- **terraform provider** — `resource "spwn_vm" "web" { name = "web", template = "node", vcores = 2 }`
- **GitHub Actions integration** — `uses: spwn/setup-vm@v1` spins up a VM for CI jobs
- **cron inside platform** — `spwn cron add <vm> "0 3 * * *" "pg_dump ..."` without needing cron inside the VM
- **event-driven scaling** — "when webhook hits this URL, start this VM" (serverless-ish)
- **Pulumi provider** — for the TypeScript crowd
- **REST hooks** — subscribe to platform events via REST polling (for environments that can't receive webhooks)

---

## storage

- **persistent volumes** — `spwn volume create mydata 10gb` → mount into any VM at any path
- **volume snapshots** — snapshot just the data volume, not the whole VM
- **shared volumes** — mount the same volume into multiple VMs (read-only or read-write with advisory locking)
- **object storage** — S3-compatible endpoint for each account. `s3://spwn/mybucket/`
- **database as a service** — `spwn db create postgres` gives you a managed postgres instance (just a VM with a template, but with backup automation)
- **volume resize** — grow a volume without stopping the VM
- **volume transfer** — move a volume between VMs or between accounts
- **local SSD tier** — option for NVMe-backed storage for latency-sensitive workloads

---

## fun & weird

- **VM uptime leaderboard** — longest-running VM gets bragging rights
- **boot time races** — "your VM booted in 127ms, that's faster than 94% of users"
- **achievement system** — "first snapshot!", "survived 30 days uptime", "used all 5 VM slots"
- **seasonal themes** — holiday themes for the dashboard (halloween, winter, etc.)
- **VM names generator** — themed name generators: space, fantasy, cyberpunk, cats
- **status page** — personal public status page showing your VMs' health at `status.spwn.run/<username>`
- **terminal recording** — record SSH sessions as asciinema casts, share with a link
- **VM stickers** — assign emoji/icons to VMs, visible in dashboard and CLI
- **sound effects** — optional terminal bell/sound on VM state changes (start → rocket sound, crash → explosion)
- **ASCII art MOTD** — auto-generated ASCII art banner when you SSH in showing VM stats
- **VM genealogy** — visual tree of which VMs were cloned from which snapshots
- **dark mode API** — `Accept: application/json+dark` returns responses with a black background (this is a joke. or is it)
- **`spwn lore`** — random lore about the platform, like `fortune` but for spwn

---

## platform & infrastructure

- **multi-region** — hosts in different datacenters, user picks region on VM create
- **edge routing** — anycast or geo-DNS so users hit the nearest region
- **ARM hosts** — graviton/ampere hosts for ARM-native VMs
- **nested virtualization** — run docker inside your VM (requires KVM passthrough in firecracker — may not be possible)
- **VM hibernation to cold storage** — VMs unused for 30+ days get snapshotted and archived. resume on next access with a 10s cold start
- **resource burst** — temporarily exceed your quota for short tasks. "I need 8 vcores for 10 minutes for this build"
- **VM scheduling** — "start this VM at 9am, stop at 6pm" for cost savings
- **maintenance windows** — user-configured windows when host maintenance (reboot, kernel update) is allowed
- **SLA tiers** — free tier: best effort. paid tier: 99.9% uptime SLA with credits
- **capacity reservations** — guarantee resources are available when you need them
- **spot VMs** — cheap VMs that can be preempted when the host needs capacity. good for batch jobs
