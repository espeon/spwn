#!/usr/bin/env bash
set -euo pipefail

for cmd in docker gh jq; do
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

echo "==> checking CI status on main"
LATEST_RUN=$(gh run list --repo "$REPO" --branch main --workflow ci --limit 1 --json status,conclusion,headSha,createdAt,databaseId)
RUN_STATUS=$(echo "$LATEST_RUN" | jq -r '.[0].status')
RUN_CONCLUSION=$(echo "$LATEST_RUN" | jq -r '.[0].conclusion')
RUN_SHA=$(echo "$LATEST_RUN" | jq -r '.[0].headSha' | head -c 8)
RUN_ID=$(echo "$LATEST_RUN" | jq -r '.[0].databaseId')

if [[ "$RUN_STATUS" != "completed" ]]; then
  echo "error: latest CI run ($RUN_ID) is still $RUN_STATUS — wait for it to finish" >&2
  echo "  https://github.com/$REPO/actions/runs/$RUN_ID" >&2
  exit 1
fi

if [[ "$RUN_CONCLUSION" != "success" ]]; then
  echo "error: latest CI run ($RUN_ID) $RUN_CONCLUSION — fix it before deploying" >&2
  echo "  commit: $RUN_SHA" >&2
  echo "  https://github.com/$REPO/actions/runs/$RUN_ID" >&2
  exit 1
fi

echo "  CI passed ($RUN_SHA) — deploying"

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
docker compose -f "$COMPOSE_FILE" pull caddy control-plane ssh-gateway grafana victoriametrics node-exporter

echo "==> restarting compose services"
docker compose -f "$COMPOSE_FILE" up -d caddy control-plane ssh-gateway grafana victoriametrics node-exporter

echo "==> deploying host-agent"
gh run download \
  --repo "$REPO" \
  --name spwn-host-agent \
  --dir "$TMPDIR"

sudo systemctl stop spwn-host-agent || true
# install the service file
if [[ ! -f /etc/systemd/system/spwn-host-agent.service ]]; then
  echo "==> installing systemd service file"
  # it's in ./spwn-host-agent.service
    sudo cp "$REPO_ROOT/deploy/spwn-host-agent.service" /etc/systemd/system/spwn-host-agent.service
    sudo systemctl daemon-reload
fi
sudo install -m 755 "$TMPDIR/spwn-host-agent" /usr/local/bin/spwn-host-agent
sudo systemctl start spwn-host-agent

echo "==> done"
docker compose -f "$COMPOSE_FILE" ps
sudo systemctl status spwn-host-agent --no-pager
