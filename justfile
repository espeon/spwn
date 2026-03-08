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

# start caddy in the background (logs to /tmp/caddy-dev.log)
caddy:
    caddy start --config config/caddy-dev.json
    @echo "caddy running — logs: /tmp/caddy-dev.log (stop with: caddy stop)"

# stop caddy
caddy-stop:
    caddy stop

# ── run ──────────────────────────────────────────────────────────────────────

# run control-plane (HTTP :3000, gRPC :5000)
cp: build-cp
    LISTEN_ADDR=0.0.0.0:3000 \
    GRPC_LISTEN_ADDR=0.0.0.0:5000 \
    STATIC_FILES_PATH=/tmp/spwn-static \
    ./target/debug/spwn-control-plane

# run host-agent (gRPC :4000) — needs root for TAP/iptables
agent: build-agent
    @command -v sudo >/dev/null 2>&1 || { echo "error: sudo not found"; exit 1; }
    @sudo -v 2>/dev/null || { echo "error: sudo credentials required — run: sudo -v"; exit 1; }
    sudo -E \
    AGENT_LISTEN_ADDR=0.0.0.0:4000 \
    AGENT_PUBLIC_ADDR=http://localhost:4000 \
    CONTROL_PLANE_URL=http://localhost:5000 \
    ./target/debug/spwn-host-agent

# ── dev setup ────────────────────────────────────────────────────────────────

# full first-time setup: download kernel + rootfs, build squashfs image
setup:
    scripts/spwn setup

# check cargo workspace compiles cleanly
check: check-protoc
    cargo check
