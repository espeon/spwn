#!/usr/bin/env bash
# First-time host provisioning for spwn host-agent.
# Run once on a fresh host before deploy.sh.
# Must be run as root (or with sudo).
set -euo pipefail

[[ $EUID -eq 0 ]] || { echo "error: must be run as root" >&2; exit 1; }

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

FC_VERSION="v1.15.1"
FC_ARCH="$(uname -m)"  # x86_64 or aarch64

info()  { echo "==> $*"; }
error() { echo "error: $*" >&2; exit 1; }

# ── deps ─────────────────────────────────────────────────────────────────────

info "installing system dependencies"
if command -v apt-get &>/dev/null; then
    apt-get install -y --no-install-recommends squashfs-tools curl ca-certificates
elif command -v dnf &>/dev/null; then
    dnf install -y squashfs-tools curl ca-certificates
elif command -v pacman &>/dev/null; then
    pacman -Sy --noconfirm squashfs-tools curl ca-certificates
else
    error "unsupported package manager — install squashfs-tools and curl manually"
fi

# ── firecracker + jailer ──────────────────────────────────────────────────────

FC_RELEASE_URL="https://github.com/firecracker-microvm/firecracker/releases/download/${FC_VERSION}"
FC_ARCHIVE="firecracker-${FC_VERSION}-${FC_ARCH}.tgz"
FC_TMP="$(mktemp -d)"
trap 'rm -rf "$FC_TMP"' EXIT

info "downloading firecracker ${FC_VERSION}"
curl -fsSL "${FC_RELEASE_URL}/${FC_ARCHIVE}" -o "${FC_TMP}/${FC_ARCHIVE}"
tar -xf "${FC_TMP}/${FC_ARCHIVE}" -C "$FC_TMP"

RELEASE_DIR="${FC_TMP}/release-${FC_VERSION}-${FC_ARCH}"
install -m 755 "${RELEASE_DIR}/firecracker-${FC_VERSION}-${FC_ARCH}" /usr/local/bin/firecracker
install -m 755 "${RELEASE_DIR}/jailer-${FC_VERSION}-${FC_ARCH}" /usr/local/bin/jailer

info "firecracker $(firecracker --version | head -1)"
info "jailer      $(jailer --version | head -1)"

# ── users + dirs ──────────────────────────────────────────────────────────────

if ! id spwn-vm &>/dev/null; then
    info "creating spwn-vm user"
    useradd -r -s /sbin/nologin spwn-vm
else
    info "spwn-vm user already exists"
fi

info "creating directories"
install -d -m 755 /var/lib/spwn
install -d -m 755 /var/lib/spwn/images
install -d -m 700 /srv/jailer

# ── kernel + rootfs ───────────────────────────────────────────────────────────

KERNEL_PATH=/var/lib/spwn/vmlinux
ROOTFS_EXT4="$(mktemp -t rootfs-XXXXXX.ext4)"
IMAGES_DIR=/var/lib/spwn/images

if [[ -f "$KERNEL_PATH" ]]; then
    info "kernel already present at $KERNEL_PATH, skipping download"
else
    info "downloading kernel"
    KERNEL_PATH="$KERNEL_PATH" \
        ROOTFS_EXT4="$ROOTFS_EXT4" \
        IMAGES_DIR="$IMAGES_DIR" \
        "$SCRIPT_DIR/scripts/spwn" download
fi

if [[ -f "$IMAGES_DIR/ubuntu.sqfs" ]]; then
    info "rootfs already present at $IMAGES_DIR/ubuntu.sqfs, skipping build"
else
    info "building squashfs rootfs"
    KERNEL_PATH="$KERNEL_PATH" \
        ROOTFS_EXT4="$ROOTFS_EXT4" \
        IMAGES_DIR="$IMAGES_DIR" \
        "$SCRIPT_DIR/scripts/spwn" build-rootfs
fi

rm -f "$ROOTFS_EXT4"

# ── done ─────────────────────────────────────────────────────────────────────

info "done. add these to /etc/spwn/env if not already set:"
echo ""
echo "  KERNEL_PATH=/var/lib/spwn/vmlinux"
echo "  ROOTFS_PATH=/var/lib/spwn/images/ubuntu.sqfs"
echo "  JAILER_BIN=/usr/local/bin/jailer"
echo "  JAILER_CHROOT_BASE=/srv/jailer"
