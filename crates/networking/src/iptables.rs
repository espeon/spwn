use std::process::Command;

use crate::{NetworkError, Result};

/// Returns the interface name of the default IPv4 route by parsing `ip route show default`.
pub fn default_route_iface() -> Result<String> {
    let output = Command::new("ip").args(["route", "show", "default"]).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // format: "default via <gw> dev <iface> ..."
    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        while let Some(token) = parts.next() {
            if token == "dev" {
                if let Some(iface) = parts.next() {
                    return Ok(iface.to_string());
                }
            }
        }
    }
    Err(NetworkError::CommandFailed {
        cmd: "ip route show default".into(),
        stderr: "no default route found".into(),
    })
}

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

    // limit concurrent TCP connections per VM IP (flood/DoS protection)
    ipt(&[
        "-A", "FORWARD",
        "-i", "fc-tap-+",
        "-p", "tcp",
        "-m", "connlimit",
        "--connlimit-above", "512",
        "--connlimit-mask", "32",
        "-j", "DROP",
    ])?;

    Ok(())
}

/// Enable IPv4 forwarding (required for NAT to work).
/// Probably a good idea to check with the user before doing this, since it affects the whole system and may have security implications.
pub fn enable_ip_forwarding() -> Result<()> {
    std::fs::write("/proc/sys/net/ipv4/ip_forward", "1").map_err(NetworkError::Io)
}

/// Helper function to run iptables commands and check for errors.
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
