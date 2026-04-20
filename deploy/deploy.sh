#!/usr/bin/env bash
set -euo pipefail

for cmd in docker gh; do
  if ! command -v "$cmd" &>/dev/null; then
    echo "error: $cmd is not installed or not in PATH" >&2
    exit 1
  fi
done

REPO="${GITHUB_REPO:-espeon/spwn}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMPOSE_FILE="$REPO_ROOT/compose.yml"
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

if [[ ! -f /etc/spwn/env ]]; then
  echo "==> installing /etc/spwn/env"
  sudo mkdir -p /etc/spwn
  sudo cp "$REPO_ROOT/.env" /etc/spwn/env
  sudo chmod 640 /etc/spwn/env
fi

if ! id spwn-vm &>/dev/null; then
  echo "==> creating spwn-vm user"
  sudo useradd -r -s /sbin/nologin spwn-vm
fi

echo "==> pulling latest images"
docker compose -f "$COMPOSE_FILE" pull caddy control-plane ssh-gateway

echo "==> restarting compose services"
docker compose -f "$COMPOSE_FILE" up -d caddy control-plane ssh-gateway

echo "==> deploying host-agent"
gh run download \
  --repo "$REPO" \
  --name spwn-host-agent \
  --dir "$TMPDIR"

sudo systemctl stop spwn-host-agent || true
sudo install -m 755 "$TMPDIR/target/release/spwn-host-agent" /usr/local/bin/spwn-host-agent
sudo systemctl start spwn-host-agent

echo "==> done"
docker compose -f "$COMPOSE_FILE" ps
sudo systemctl status spwn-host-agent --no-pager
