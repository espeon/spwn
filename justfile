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
