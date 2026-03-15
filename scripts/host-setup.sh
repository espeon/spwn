#!/usr/bin/env bash
# spwn host node setup — run once as root before starting host-agent.
# Idempotent: safe to re-run.
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
  echo "error: must run as root" >&2
  exit 1
fi

info()  { echo "[setup] $*"; }
warn()  { echo "[warn]  $*" >&2; }

# ---------------------------------------------------------------------------
# 1. Required packages
# ---------------------------------------------------------------------------
info "checking required packages"
MISSING=()
for bin in firecracker jailer iptables tc ip; do
  command -v "$bin" &>/dev/null || MISSING+=("$bin")
done
if [[ ${#MISSING[@]} -gt 0 ]]; then
  warn "missing binaries: ${MISSING[*]}"
  warn "install firecracker/jailer from https://github.com/firecracker-microvm/firecracker/releases"
fi

# ---------------------------------------------------------------------------
# 2. Dedicated jailer user/group
# ---------------------------------------------------------------------------
if ! id spwn-vm &>/dev/null; then
  info "creating spwn-vm user"
  useradd -r -s /sbin/nologin spwn-vm
else
  info "spwn-vm user already exists"
fi

# ---------------------------------------------------------------------------
# 3. Runtime directories
# ---------------------------------------------------------------------------
info "creating runtime directories"
for dir in /var/lib/spwn /var/lib/spwn/images /var/lib/spwn/overlays /var/lib/spwn/snapshots /srv/jailer; do
  mkdir -p "$dir"
done

# ---------------------------------------------------------------------------
# 4. Disable KSM (memory deduplication side-channel)
# ---------------------------------------------------------------------------
info "disabling KSM"
if [[ -f /sys/kernel/mm/ksm/run ]]; then
  echo 0 > /sys/kernel/mm/ksm/run
  # persist across reboots via udev/sysctl
  if [[ -d /etc/sysctl.d ]]; then
    echo "kernel.mm.ksm.run=0" > /etc/sysctl.d/99-spwn-ksm.conf
  fi
else
  warn "KSM not available (kernel compiled without CONFIG_KSM) — skipping"
fi

# ---------------------------------------------------------------------------
# 5. SMT (Hyper-Threading) check
# ---------------------------------------------------------------------------
info "checking SMT state"
if [[ -f /sys/devices/system/cpu/smt/active ]]; then
  SMT_ACTIVE=$(cat /sys/devices/system/cpu/smt/active)
  if [[ "$SMT_ACTIVE" == "1" ]]; then
    warn "SMT is enabled — for multi-tenant production, disable it:"
    warn "  echo off > /sys/devices/system/cpu/smt/control"
    warn "  add 'nosmt' to GRUB_CMDLINE_LINUX for persistence"
  else
    info "SMT is disabled"
  fi
else
  info "SMT control not available (single-core or unsupported CPU)"
fi

# ---------------------------------------------------------------------------
# 6. Swap
# ---------------------------------------------------------------------------
info "checking swap"
SWAP_LINES=$(awk 'NR>1' /proc/swaps | wc -l)
if [[ "$SWAP_LINES" -gt 0 ]]; then
  warn "swap is active — disabling now (swapoff -a)"
  swapoff -a
  warn "to persist, comment out swap entries in /etc/fstab"
else
  info "no active swap"
fi

# ---------------------------------------------------------------------------
# 7. KVM tuning
# ---------------------------------------------------------------------------
info "applying KVM tuning"
KVM_PARAMS=/sys/module/kvm/parameters
if [[ -f "$KVM_PARAMS/nx_huge_pages" ]]; then
  echo never > "$KVM_PARAMS/nx_huge_pages"
  info "  nx_huge_pages=never"
fi
# min_timer_period_us reduces boot latency — 20 µs is the Firecracker recommendation
if [[ -f "$KVM_PARAMS/min_timer_period_us" ]]; then
  echo 20 > "$KVM_PARAMS/min_timer_period_us" 2>/dev/null || true
  info "  min_timer_period_us=20"
fi

# ---------------------------------------------------------------------------
# 8. cgroupsv2 — remount with favordynmods (Linux 6.1+ regression fix)
# ---------------------------------------------------------------------------
info "checking cgroupsv2 mount options"
CGROUP_MOUNT=$(findmnt -n -o OPTIONS /sys/fs/cgroup 2>/dev/null || true)
if [[ -n "$CGROUP_MOUNT" ]] && [[ "$CGROUP_MOUNT" != *"favordynmods"* ]]; then
  # Try to remount; this can fail on some distros — non-fatal.
  mount -o remount,favordynmods /sys/fs/cgroup 2>/dev/null \
    && info "  remounted cgroupsv2 with favordynmods" \
    || warn "  could not remount cgroupsv2 with favordynmods — consider adding to fstab"
else
  info "  cgroupsv2 already has favordynmods or mount check skipped"
fi

# ---------------------------------------------------------------------------
# 9. IPv4 forwarding
# ---------------------------------------------------------------------------
info "enabling IPv4 forwarding"
echo 1 > /proc/sys/net/ipv4/ip_forward
if [[ -d /etc/sysctl.d ]]; then
  echo "net.ipv4.ip_forward=1" > /etc/sysctl.d/99-spwn-forward.conf
fi

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------
info ""
info "host setup complete."
info "remaining manual steps:"
info "  - install kernel + rootfs at \$KERNEL_PATH / \$ROOTFS_PATH"
info "  - set FIRECRACKER_BIN, JAILER_BIN, DATABASE_URL, AGENT_PUBLIC_ADDR in .env"
info "  - if SMT is still on, add 'nosmt' to kernel cmdline and reboot"
