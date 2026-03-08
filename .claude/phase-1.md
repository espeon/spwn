# phase 1: firecracker spike

**goal:** boot a single VM via fctools, verify TAP networking + iptables, SSH in from host.

**done when:**
- `ssh user@172.16.0.2` connects from host
- VM has outbound internet (`curl https://example.com` inside guest)
- VM cannot reach host :3000, :2019, :22 (iptables DROP verified with `curl`/`nc`)

---

## status

- [x] cargo workspace with all crate skeletons
- [x] `common` crate: `VmId`, basic types
- [x] `networking` crate: TAP creation, IP allocation, iptables, `NetworkManager`
- [x] `vm-manager/src/main.rs`: spike binary compiles clean against fctools 0.7.0-alpha.1
- [x] firecracker binary installed at `~/.local/bin/firecracker`
- [x] spike actually boots a VM (runtime validation)
- [x] SSH into guest works (use CI key — see below)
- [ ] iptables DROP rules verified (deferred — good enough for now)

---

## what the spike does (`crates/vm-manager/src/main.rs`)

1. calls `iptables::enable_ip_forwarding()` + `iptables::setup(&external_iface)`
2. allocates TAP device via `NetworkManager::allocate_tap`
3. constructs `VmmInstallation` + `UnrestrictedVmmExecutor` (no jailer yet — see note below)
4. creates `ResourceSystem` with `VmmOwnershipModel::Shared`
5. registers kernel + rootfs as `Moved(HardLinkedOrCopied)` resources
6. builds `VmConfiguration::New` with boot args, drives, machine config, network interface
7. `Vm::prepare` → `vm.start` → wait for enter → `vm.shutdown` → `vm.cleanup`

**jailer note:** spike intentionally uses `UnrestrictedVmmExecutor` (no jailer) to reduce iteration friction. jailer integration is phase 3 work when we have the full lifecycle API. the iptables DROP rules still apply and should be tested now.

---

## env vars for running the spike

```bash
export KERNEL_PATH=/tmp/vmlinux
export ROOTFS_PATH=/tmp/rootfs.ext4
export EXTERNAL_IFACE=eth0   # or whatever your host's outbound iface is
# optional:
export FIRECRACKER_BIN=/usr/local/bin/firecracker

sudo -E cargo run -p vm-manager
```

download test artifacts if not present:
```bash
curl -Lo /tmp/vmlinux https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/v1.9/x86_64/vmlinux-6.1.102
curl -Lo /tmp/rootfs.ext4 https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/v1.9/x86_64/ubuntu-22.04.ext4
```

---

## verification checklist

```bash
# on host — after spike prints "VM running":
ip addr show fc-tap-spike-0        # TAP device exists with 172.16.0.1/30
ping -c 1 172.16.0.2               # guest responds

# SSH (rootfs default creds for ubuntu-22.04.ext4 are root / no password or root/root)
ssh root@172.16.0.2

# inside guest:
curl -s https://example.com | head -5    # outbound internet works
nc -zv 172.16.0.1 3000 || echo "blocked" # host :3000 blocked
nc -zv 172.16.0.1 2019 || echo "blocked" # host :2019 blocked
nc -zv 172.16.0.1 22   || echo "blocked" # host SSH blocked
```

---

## known rough edges

- **rootfs is copied/hard-linked** into fctools' managed environment on `Vm::prepare`. if you're iterating fast, the source file at `/tmp/rootfs.ext4` is preserved (hard link on same FS, copy across devices).
- **socket path** is `/tmp/fc-spike-0.sock` — remove it if the spike crashes before cleanup: `rm -f /tmp/fc-spike-0.sock`
- **orphaned TAP devices** after crash: `sudo ip tuntap del dev fc-tap-spike-0 mode tap`
- **ip forwarding** may already be on; `iptables::enable_ip_forwarding` is idempotent (writes `1` to `/proc/sys/net/ipv4/ip_forward`)

---

## what's NOT in scope for phase 1

- jailer (phase 3)
- sqlite / db crate (phase 3)
- reconciliation loop (phase 3)
- caddy routing (phase 2)
- any API server (phase 3)
