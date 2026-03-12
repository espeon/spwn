#!/bin/sh
set -e

# Detect distro from /etc/os-release and install the minimum packages needed
# for a VM to boot: an init system (systemd or openrc) + openssh-server + basics.
#
# Supported package managers: apt, apk, dnf, pacman, zypper.
# For anything else, skip gracefully and let the caller handle it.

if [ ! -f /etc/os-release ]; then
    echo "[rootfs-config] no /etc/os-release found, skipping package install" >&2
    exit 0
fi

. /etc/os-release

install_apt() {
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y -qq \
        systemd \
        systemd-sysv \
        dbus \
        ca-certificates \
        curl \
        wget \
        git \
        vim \
        htop \
        jq \
        unzip \
        build-essential \
        openssh-server
    apt-get clean
    rm -rf /var/lib/apt/lists/*
}

install_apk() {
    apk update
    apk add --no-cache \
        openrc \
        openssh \
        ca-certificates \
        curl \
        wget \
        git \
        vim \
        htop \
        jq \
        unzip \
        build-base
    # enable sshd on boot
    rc-update add sshd default 2>/dev/null || true
}

install_dnf() {
    local pm=dnf
    command -v dnf >/dev/null 2>&1 || pm=yum
    $pm install -y \
        systemd \
        dbus \
        ca-certificates \
        curl \
        wget \
        git \
        vim-enhanced \
        htop \
        jq \
        unzip \
        gcc \
        gcc-c++ \
        make \
        openssh-server
    $pm clean all
    systemctl enable sshd 2>/dev/null || true
}

install_pacman() {
    pacman -Sy --noconfirm \
        systemd \
        dbus \
        ca-certificates \
        curl \
        wget \
        git \
        vim \
        htop \
        jq \
        unzip \
        base-devel \
        openssh
    pacman -Sc --noconfirm
    systemctl enable sshd 2>/dev/null || true
}

install_zypper() {
    zypper --non-interactive refresh
    zypper --non-interactive install \
        systemd \
        dbus-1 \
        ca-certificates \
        curl \
        wget \
        git \
        vim \
        htop \
        jq \
        unzip \
        gcc \
        gcc-c++ \
        make \
        openssh
    zypper --non-interactive clean
    systemctl enable sshd 2>/dev/null || true
}

harden_sshd() {
    local cfg=/etc/ssh/sshd_config
    [ -f "$cfg" ] || return
    sed -i 's/^#\?PermitRootLogin.*/PermitRootLogin prohibit-password/' "$cfg"
    sed -i 's/^#\?PasswordAuthentication.*/PasswordAuthentication no/' "$cfg"
}

# ID_LIKE lets a derivative (e.g. ubuntu) match its parent (debian).
# Check ID first, then fall back to ID_LIKE.
detect_and_install() {
    for id in $ID $ID_LIKE; do
        case "$id" in
            debian|ubuntu|linuxmint|pop|elementary|kali|raspbian)
                echo "[rootfs-config] detected apt-based distro ($id)"
                install_apt
                return
                ;;
            alpine)
                echo "[rootfs-config] detected apk-based distro ($id)"
                install_apk
                return
                ;;
            fedora|rhel|centos|rocky|almalinux|ol|amzn)
                echo "[rootfs-config] detected dnf/yum-based distro ($id)"
                install_dnf
                return
                ;;
            arch|manjaro|endeavouros|garuda)
                echo "[rootfs-config] detected pacman-based distro ($id)"
                install_pacman
                return
                ;;
            opensuse*|suse|sles)
                echo "[rootfs-config] detected zypper-based distro ($id)"
                install_zypper
                return
                ;;
        esac
    done

    echo "[rootfs-config] unrecognised distro (ID=$ID ID_LIKE=${ID_LIKE:-}), skipping package install" >&2
    echo "[rootfs-config] ensure your image has an init system and openssh-server installed" >&2
}

detect_and_install
harden_sshd
