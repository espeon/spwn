# spwn dev justfile
# usage: just <recipe>
# requires: just, cargo, protoc, podman/docker, caddy

set dotenv-load := true

# show available recipes
default:
    @just --list

# ── prereqs ──────────────────────────────────────────────────────────────────

# check protoc is installed (required for agent-proto build)
check-protoc:
    @command -v protoc >/dev/null 2>&1 || { \
        echo "error: protoc not found. install with:"; \
        echo "  arch:   sudo pacman -S protobuf"; \
        echo "  debian: sudo apt install protobuf-compiler"; \
        exit 1; \
    }
    @echo "protoc $(protoc --version)"

# ── build ────────────────────────────────────────────────────────────────────

# build control-plane binary
build-cp: check-protoc
    cargo build -p control-plane

# build host-agent binary
build-agent: check-protoc
    cargo build -p host-agent

# build both
build: check-protoc
    cargo build -p control-plane -p host-agent

# ── infrastructure ────────────────────────────────────────────────────────────

# run cloudflare tunnel (requires CLOUDFLARE_TUNNEL_TOKEN in .env)
tunnel:
    podman run --rm --network=host docker.io/cloudflare/cloudflared:latest \
        tunnel --no-autoupdate run --token "$CLOUDFLARE_TUNNEL_TOKEN"

# start postgres via podman compose in the background
pg:
    podman compose up -d postgres

# stop postgres
pg-stop:
    podman compose stop postgres

# wipe and restart postgres, then run migrations via the control-plane binary
pg-reset:
    podman compose down -v
    podman compose up -d postgres
    @echo "waiting for postgres to accept connections..."
    @until podman compose exec postgres pg_isready -U $POSTGRES_USER -d $POSTGRES_DB -q 2>/dev/null; do sleep 1; done
    @echo "postgres ready — run 'just cp' to start the control-plane and apply migrations"

# start caddy in the background (logs to /tmp/caddy-dev.log)
caddy:
    caddy start --config config/caddy-dev.json
    @echo "caddy running — logs: /tmp/caddy-dev.log (stop with: caddy stop)"

# stop caddy
caddy-stop:
    caddy stop

# ── run ──────────────────────────────────────────────────────────────────────

# run control-plane (configure via .env: LISTEN_ADDR, GRPC_LISTEN_ADDR, STATIC_FILES_PATH, INVITE_CODE)
cp: build-cp
    ./target/debug/spwn-control-plane

# run host-agent (configure via .env: AGENT_LISTEN_ADDR, AGENT_PUBLIC_ADDR, CONTROL_PLANE_URL)
agent: build-agent
    @command -v sudo >/dev/null 2>&1 || { echo "error: sudo not found"; exit 1; }
    @sudo -v 2>/dev/null || { echo "error: sudo credentials required — run: sudo -v"; exit 1; }
    sudo -E ./target/debug/spwn-host-agent

# ── frontend ──────────────────────────────────────────────────────────────────

# build frontend for production (output: frontend/dist)
frontend-build:
    cd frontend && pnpm build

# run vite dev server (proxies /auth and /api to localhost:3000)
frontend:
    cd frontend && pnpm dev --host

# ── dev setup ────────────────────────────────────────────────────────────────

# full first-time setup: download kernel + rootfs, build squashfs image
setup:
    scripts/spwn setup

# run tests (sets DOCKER_HOST for testcontainers + podman)
test:
    DOCKER_HOST=unix:///run/user/$(id -u)/podman/podman.sock \
    TESTCONTAINERS_RYUK_DISABLED=true \
    cargo test -p db -p auth

# check cargo workspace compiles cleanly
check: check-protoc
    cargo check

# ── spwn CLI / TUI ────────────────────────────────────────────────────────────

# build spwn CLI binary (output: target/spwn)
spwn-build:
    cd services && go build -o ../target/spwn ./cmd/spwn

# build ssh-gateway binary (output: target/spwn-ssh-gateway)
ssh-gateway-build:
    cd services && go build -o ../target/spwn-ssh-gateway ./cmd/ssh-gateway

# build both Go binaries
go-build: spwn-build ssh-gateway-build

# run spwn TUI (builds first)
spwn: spwn-build
    ./target/spwn

# run ssh-gateway (configure via .env: SSH_GATEWAY_LISTEN_ADDR, SSH_GATEWAY_HOST_KEY_PATH, GATEWAY_SECRET, CONTROL_PLANE_HTTP_URL)
gateway: ssh-gateway-build
    ./target/spwn-ssh-gateway

# tidy Go deps
go-tidy:
    cd services && go mod tidy
