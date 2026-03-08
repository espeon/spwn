use std::process::Command;

use crate::{NetworkError, Result};

/// Set up iptables rules for VM networking:
/// - NAT masquerade for outbound internet
/// - FORWARD rules for TAP ↔ external interface
/// - DROP rules to block VMs from reaching host services
///
/// `external_iface`: the host's external network interface (e.g. "eth0")
pub fn setup(external_iface: &str) -> Result<()> {
    // outbound NAT
    ipt(&["-t", "nat", "-A", "POSTROUTING", "-s", "172.16.0.0/16", "-o", external_iface, "-j", "MASQUERADE"])?;

    // allow VMs to forward to internet
    ipt(&["-A", "FORWARD", "-i", "fc-tap-+", "-o", external_iface, "-j", "ACCEPT"])?;
    ipt(&["-A", "FORWARD", "-i", external_iface, "-o", "fc-tap-+", "-m", "state", "--state", "RELATED,ESTABLISHED", "-j", "ACCEPT"])?;

    // block VMs from reaching host services
    for port in ["3000", "2019", "22"] {
        ipt(&["-A", "INPUT", "-s", "172.16.0.0/16", "-p", "tcp", "--dport", port, "-j", "DROP"])?;
    }

    Ok(())
}

/// Enable IPv4 forwarding (required for NAT to work).
pub fn enable_ip_forwarding() -> Result<()> {
    std::fs::write("/proc/sys/net/ipv4/ip_forward", "1").map_err(NetworkError::Io)
}

fn ipt(args: &[&str]) -> Result<()> {
    let output = Command::new("iptables").args(args).output()?;
    if !output.status.success() {
        return Err(NetworkError::CommandFailed {
            cmd: format!("iptables {}", args.join(" ")),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        });
    }
    Ok(())
}
