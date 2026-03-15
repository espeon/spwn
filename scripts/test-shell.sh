#!/usr/bin/env bash
# test-shell.sh — create a throwaway VM from an image and SSH into it
#
# Usage:
#   SPWN_TOKEN=<token> scripts/test-shell.sh [image]
#   SPWN_TOKEN=<token> scripts/test-shell.sh ubuntu:22.04
#
# The VM is deleted automatically when you exit the shell.
#
# Required env:
#   SPWN_TOKEN          — API bearer token (from login)
#
# Optional env:
#   SPWN_API            — control-plane base URL (default: http://localhost:3019)
#   PLATFORM_KEY_PATH   — SSH private key (default: ~/.ssh/id_rsa)

set -uo pipefail

IMAGE="${1:-ubuntu}"
API="${SPWN_API:-http://localhost:3019}"
PLATFORM_KEY="${PLATFORM_KEY_PATH:-$HOME/.ssh/id_rsa}"
VM_ID=""

die() { echo "error: $*" >&2; exit 1; }

apicurl() {
    local url="$1"; shift
    curl -s -w "\n%{http_code}" -H "Authorization: Bearer $SPWN_TOKEN" "$@" "$url"
}

api_check() {
    local label="$1" body="$2" code="$3"
    if [[ "$code" -lt 200 || "$code" -ge 300 ]]; then
        die "$label failed (HTTP $code): $body"
    fi
}

extract() {
    # extract a JSON string field by key name; no jq dependency
    local key="$1" json="$2"
    echo "$json" | grep -oP "\"${key}\"\s*:\s*\"\K[^\"]+" | head -1
}

[[ -z "${SPWN_TOKEN:-}" ]] && die "SPWN_TOKEN is required"
[[ ! -f "$PLATFORM_KEY" ]] && die "SSH key not found at $PLATFORM_KEY (set PLATFORM_KEY_PATH)"

cleanup() {
    if [[ -n "$VM_ID" ]]; then
        echo ""
        echo "=> deleting VM $VM_ID ..."
        curl -sf -X DELETE "$API/api/vms/$VM_ID" \
            -H "Authorization: Bearer $SPWN_TOKEN" > /dev/null 2>&1 || true
        echo "=> done"
    fi
}
trap cleanup EXIT INT TERM

echo "=> creating VM from image '$IMAGE' ..."
RESP=$(apicurl "$API/api/vms" \
    -X POST \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"test-shell-$$\",\"image\":\"$IMAGE\"}")
BODY="${RESP%$'\n'*}"
CODE="${RESP##*$'\n'}"
api_check "create VM" "$BODY" "$CODE"

VM_ID=$(extract "id" "$BODY")
[[ -z "$VM_ID" ]] && die "could not parse VM id from response: $BODY"
echo "=> VM created: $VM_ID"

echo "=> starting VM ..."
RESP=$(apicurl "$API/api/vms/$VM_ID/start" -X POST)
BODY="${RESP%$'\n'*}"
CODE="${RESP##*$'\n'}"
api_check "start VM" "$BODY" "$CODE"

echo "=> waiting for VM to be running ..."
STATUS=""
for i in $(seq 1 60); do
    RESP=$(apicurl "$API/api/vms/$VM_ID")
    BODY="${RESP%$'\n'*}"
    CODE="${RESP##*$'\n'}"
    if [[ "$CODE" -lt 200 || "$CODE" -ge 300 ]]; then
        die "poll VM status failed (HTTP $CODE): $BODY"
    fi
    STATUS=$(extract "status" "$BODY")
    if [[ "$STATUS" == "running" ]]; then
        break
    fi
    if [[ "$STATUS" == "error" || "$STATUS" == "stopped" ]]; then
        die "VM entered status '$STATUS'"
    fi
    sleep 1
done

[[ "$STATUS" != "running" ]] && die "VM did not reach running state after 60s (status: $STATUS)"

IP=$(extract "ip_address" "$BODY")
[[ -z "$IP" ]] && die "could not parse VM IP from response: $BODY"

echo "=> VM running at $IP"
echo "=> connecting via SSH (exit to delete the VM) ..."
echo ""

ssh \
    -i "$PLATFORM_KEY" \
    -o StrictHostKeyChecking=no \
    -o UserKnownHostsFile=/dev/null \
    -o LogLevel=ERROR \
    root@"$IP"
