#!/usr/bin/env bash
set -euo pipefail

COMPOSE_FILE="$(dirname "$0")/../compose.yml"

echo "==> pulling latest images"
docker compose -f "$COMPOSE_FILE" pull caddy control-plane ssh-gateway

echo "==> restarting services"
docker compose -f "$COMPOSE_FILE" up -d caddy control-plane ssh-gateway

echo "==> deploying host-agent"
gh run download \
  --repo "${GITHUB_REPO:-espeon/spwn}" \
  --name spwn-host-agent \
  --dir /tmp/spwn-deploy

sudo systemctl stop spwn-host-agent || true
sudo install -m 755 /tmp/spwn-host-agent/host-agent /usr/local/bin/spwn-host-agent
sudo systemctl start spwn-host-agent
rm -rf /tmp/spwn-host-agent

echo "==> done"
docker compose -f "$COMPOSE_FILE" ps
