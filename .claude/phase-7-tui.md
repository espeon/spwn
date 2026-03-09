# phase 7: management TUI + CLI

**goal:** a `spwn` binary (Go + charm) that doubles as an interactive terminal management UI and a scriptable CLI. auth uses a browser-based device flow — no passwords in the terminal.

---

## project structure: `services/`

one Go module covering both binaries. shared packages live at the top level; each binary has its own `cmd/` entry point.

```
services/
  go.mod                     module: github.com/you/spwn/services
  go.sum
  cmd/
    spwn/
      main.go                → spwn binary (CLI + local TUI, no SSH server)
    ssh-gateway/
      main.go                → ssh-gateway binary (wish server, operator-run)
  tui/
    app.go                   root bubbletea model + router between views
    vm_list.go               VM table, live status, keyboard actions
    vm_detail.go             single VM: events, snapshots, resource bars
    snapshot_detail.go       single snapshot: metadata, restore/delete actions
    account.go               quota bars, account info
    new_vm.go                inline form for VM creation
    confirm.go               generic confirmation dialog
    styles.go                lip gloss style definitions
    keys.go                  keybinding definitions
  client/
    client.go                HTTP client, bearer token auth
    types.go                 shared API response types
  ssh/                       only ssh-gateway imports this
    server.go                wish SSH server setup
    auth.go                  pubkey + password auth via gRPC
    router.go                username → VM lookup → session dispatch
```

deps: bubbletea, lipgloss, bubbles, wish (ssh-gateway only), cobra (CLI), tablewriter or lipgloss table (CLI output), `pkg/browser` (open browser for auth flow)

---

## auth: browser device flow

`spwn login` never asks for a password. instead:

```
1. POST /auth/cli/init
   ← { code: "abc123", browser_url: "https://spwn.dev/cli-auth?code=abc123", expires_in: 300 }

2. CLI opens browser_url via pkg/browser

3. CLI polls GET /auth/cli/poll?code=abc123 every 2s (up to expires_in)
   ← { status: "pending" }
   ← { status: "authorized", token: "spwn_tok_..." }
   ← { status: "denied" }
   ← { status: "expired" }

4. CLI stores token in ~/.config/spwn/credentials (mode 0600)
   all future requests: Authorization: Bearer <token>
```

the web frontend handles the user-facing side of this at `/cli-auth?code=<code>`.

### why not a local redirect server

local port listener (e.g. `localhost:PORT/callback`) is more fragile — firewalls, port conflicts, browser security policies. polling is simpler to reason about, works everywhere, and expires cleanly.

---

## control-plane changes

### new DB tables

```sql
CREATE TABLE cli_auth_codes (
    code        TEXT PRIMARY KEY,
    account_id  TEXT REFERENCES accounts(id) ON DELETE CASCADE,
    status      TEXT NOT NULL DEFAULT 'pending',  -- pending | authorized | denied | expired
    expires_at  BIGINT NOT NULL
);

CREATE TABLE api_tokens (
    id           TEXT PRIMARY KEY,
    account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    token_hash   TEXT NOT NULL UNIQUE,   -- sha256 of the raw token
    name         TEXT NOT NULL,
    created_at   BIGINT NOT NULL,
    last_used_at BIGINT
);
```

### new auth routes

```
POST /auth/cli/init                    create pending code; return code + browser_url
GET  /auth/cli/poll?code=<code>        check status; return token on authorized
POST /auth/cli/authorize               (session-protected) approve code → create api_token
POST /auth/cli/deny                    (session-protected) deny code
```

### bearer token auth

all existing session-cookie-protected routes also accept `Authorization: Bearer <token>`:
- add `BearerAuth` extractor that resolves to `AccountId` via `api_tokens` lookup
- combine with existing `SessionAuth` via a union extractor (`AuthedUser`)

### name-based VM lookup + events

```
GET  /api/vms?name=<name>             lookup VM by name
GET  /api/vms/:id/events              paginated event log (cursor-based)
PATCH /api/vms/:id                    rename, update exposed_port
```

---

## frontend changes

### `/cli-auth` page

```
/cli-auth?code=<code>
```

- not logged in → redirect to `/login?next=/cli-auth?code=<code>`
- logged in → minimal approval card (no sidebar, no layout chrome):
  - "authorize CLI access as nat@spwn.dev?"
  - [Authorize] → `POST /auth/cli/authorize { code }` → success screen
  - [Deny] → `POST /auth/cli/deny { code }` → denied screen
  - countdown timer to code expiry

---

## TUI design

### launching

```
spwn              → TUI (interactive, if stdout is a TTY)
spwn vm list      → plain table output
spwn vm start foo → runs and exits
```

TTY detection: if `!term.IsTerminal(int(os.Stdout.Fd()))`, always plain CLI regardless of args.

### VM list view (default)

```
 spwn                                              nat@spwn.dev
 ─────────────────────────────────────────────────────────────
  VMs                                              [n] new

  ▶  vivid-moon-be33   ● running    2c  2.0gb   vm.spwn.dev
     quick-fox-a1b2    ○ stopped    1c  512mb   fox.spwn.dev
     dark-star-ff01    ● running    4c  4.0gb   dark.spwn.dev

 ─────────────────────────────────────────────────────────────
  quota: 7/8 vcores │ 6.5/12gb ram │ 3/5 vms
  [↑↓/jk] navigate  [enter] detail  [s] start/stop  [d] delete  [?] help  [q] quit
```

polls `GET /api/vms` every 5s. status color: green=running, yellow=starting/stopping, dim=stopped, red=error.

### VM detail view

```
 spwn / vivid-moon-be33                            nat@spwn.dev
 ─────────────────────────────────────────────────────────────
  ● running   2 vcores   2.0gb ram   vm.spwn.dev

  Recent events                     Snapshots
  ─────────────────                 ──────────────────────────
  started          2m ago           snap-a1b2   3h ago   [enter]
  snapshot taken   3h ago           snap-ff01   1d ago   [enter]
  created          2d ago

 ─────────────────────────────────────────────────────────────
  [s] stop  [p] snapshot  [r] rename  [d] delete  [esc] back
```

snapshots are navigable — pressing enter on one goes to snapshot detail.

### snapshot detail view

```
 spwn / vivid-moon-be33 / snap-a1b2                nat@spwn.dev
 ─────────────────────────────────────────────────────────────
  snap-a1b2

  label      before-experiment
  taken      3 hours ago (2026-03-09 14:32)
  vm state   was running at snapshot time

 ─────────────────────────────────────────────────────────────
  [r] restore  [d] delete  [esc] back
```

honestly not a ton to show here — snapshot metadata is thin (label, timestamp, maybe vm state at time of snap). the value is having a dedicated view with restore/delete actions without cluttering the VM detail.

### account view

```
 spwn / account                                    nat@spwn.dev
 ─────────────────────────────────────────────────────────────
  nat@spwn.dev

  vcores  ████████░░  7/8
  ram     ███████░░░  6.5/12gb
  vms     ██████░░░░  3/5

 ─────────────────────────────────────────────────────────────
  [esc] back
```

### new VM form (inline overlay)

```
  Name:    [_______________]
  vCores:  [2]
  Memory:  [2048] mb

  [enter] create  [esc] cancel
```

### key bindings

| key       | action                           |
| --------- | -------------------------------- |
| j / ↓     | move down                        |
| k / ↑     | move up                          |
| enter     | select / confirm                 |
| esc       | back / cancel                    |
| s         | start/stop selected VM           |
| n         | new VM                           |
| d         | delete (with confirmation)       |
| r         | rename                           |
| p         | take snapshot                    |
| a         | account view                     |
| ?         | toggle help overlay              |
| q         | quit (from root view)            |

---

## CLI commands (for scripting)

```
spwn login                          browser device flow → stores token
spwn logout                         clear stored token
spwn whoami                         show account + quota

spwn vm list                        table: name, subdomain, status, vcores, mem
spwn vm create <name>               --vcores N  --memory N (mb)
spwn vm start <name|id>
spwn vm stop <name|id>
spwn vm delete <name|id>            --force to skip confirmation
spwn vm status <name|id>            detailed status + recent events
spwn vm rename <name|id> <new>

spwn snapshot list <vm>
spwn snapshot take <vm>             --label
spwn snapshot restore <vm> <snap>
spwn snapshot delete <vm> <snap>

spwn ssh <vm>                       stub: prints instructions (phase 8)
spwn ssh <vm> -- <cmd>              stub
spwn tunnel <vm> <local>:<remote>   stub

spwn keys list                      prep for phase 8 SSH gateway
spwn keys add <name> <path>
spwn keys add <name> -              read from stdin
spwn keys remove <name|id>

spwn config set <key> <value>
spwn config get <key>
```

output flags on all commands:
- `--json`    machine-readable JSON
- `--quiet`   suppress non-essential output

---

## deliverables

### auth + token infra (control-plane)

- [ ] `cli_auth_codes` + `api_tokens` DB migrations
- [ ] `POST /auth/cli/init`, `GET /auth/cli/poll`, `POST /auth/cli/authorize`, `POST /auth/cli/deny`
- [ ] bearer token extraction alongside session cookie auth (`AuthedUser` union extractor)
- [ ] `/cli-auth` frontend page (approve/deny, countdown, post-action screen)

### Go module setup

- [ ] `services/go.mod` with bubbletea, lipgloss, bubbles, cobra, pkg/browser
- [ ] `cmd/spwn/main.go` + `cmd/ssh-gateway/main.go` entry points
- [ ] `client/` HTTP client with bearer token support
- [ ] justfile recipes: `just spwn-build`, `just spwn` (build + run)

### CLI commands

- [ ] cobra command tree with all subcommands
- [ ] config: `~/.config/spwn/config.toml` (api-url, default-vcores, default-memory)
- [ ] credentials: `~/.config/spwn/credentials` (mode 0600)
- [ ] `spwn login` browser device flow + poll loop
- [ ] `spwn logout` / `spwn whoami`
- [ ] full VM lifecycle commands with table output
- [ ] snapshot commands
- [ ] `spwn ssh` / `spwn tunnel` stubs
- [ ] `spwn keys` commands
- [ ] `--json` + `--quiet` flags on all commands
- [ ] colored status in table output (lipgloss)
- [ ] shell completions (cobra built-in: bash/zsh/fish/powershell)

### TUI

- [ ] bubbletea event loop with TTY detection
- [ ] VM list view with 5s polling
- [ ] VM detail view (events + snapshot list)
- [ ] snapshot detail view (metadata + restore/delete)
- [ ] account view (quota progress bars)
- [ ] new VM inline form
- [ ] delete confirmation dialog
- [ ] rename inline edit
- [ ] help overlay (`?`)
- [ ] start/stop spinner (optimistic UI until status changes)

### control-plane API additions

- [ ] `GET /api/vms?name=<name>`
- [ ] `GET /api/vms/:id/events` (cursor-based pagination)
- [ ] `PATCH /api/vms/:id`

---

## phase 8 relationship

the `services/ssh-gateway/` binary imports `services/tui/` for its bubbletea views — the dashboard TUI that appears when a user SSHes in reuses the same models as the local `spwn` TUI. phase 8 adds `services/ssh/` (wish server, gRPC auth) and wires it up; phase 7 doesn't touch that package.

---

## deferred

- `spwn ssh` / `spwn tunnel` fully working (phase 8)
- VM console streaming in TUI (phase 8)
- API token management UI in frontend
- man page generation
