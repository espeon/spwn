use std::net::Ipv4Addr;
use std::process::Command;

use crate::{NetworkError, Result, ip};

// Apply tc tbf (token bucket filter) shaping to a TAP device.
// Sets an egress rate cap (host→VM direction) which is the primary
// noisy-neighbour vector. burst is set to 1.5x the per-10ms quota,
// giving headroom for short bursts without allowing sustained overuse.
// latency is 50ms — packets queued beyond this are dropped.
pub fn apply_tc_shaping(tap: &str, mbps: u32) -> Result<()> {
    let rate = format!("{mbps}mbit");
    let burst_bytes = (mbps as u64 * 1_000_000 / 8 / 100) * 3 / 2;
    let burst = format!("{burst_bytes}b");

    // Clear any existing qdisc first (idempotent).
    let _ = Command::new("tc")
        .args(["qdisc", "del", "dev", tap, "root"])
        .output();

    let output = Command::new("tc")
        .args([
            "qdisc", "add", "dev", tap, "root", "tbf", "rate", &rate, "burst", &burst, "latency",
            "50ms",
        ])
        .output()?;

    if !output.status.success() {
        return Err(NetworkError::CommandFailed {
            cmd: format!("tc qdisc add dev {tap} root tbf rate {rate}"),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        });
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct TapDevice {
    pub name: String,
    pub host_ip: Ipv4Addr,
    pub guest_ip: Ipv4Addr,
    pub slot: u32,
}

pub struct NetworkManager;

impl NetworkManager {
    pub fn new() -> Self {
        Self
    }

    /// Create a TAP device for a VM at `slot` in the 172.16.0.0/16 space.
    /// If a stale device from a previous run exists at this slot, it is removed first.
    pub fn allocate_tap(&self, slot: u32) -> Result<TapDevice> {
        let name = tap_name(slot);
        let host_ip = ip::host_ip(slot);
        let guest_ip = ip::guest_ip(slot);

        let already_exists = Command::new("ip")
            .args(["link", "show", &name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if already_exists {
            run("ip", &["link", "delete", &name]).map_err(|e| NetworkError::CommandFailed {
                cmd: format!("remove stale tap {name}"),
                stderr: e.to_string(),
            })?;
        }

        run("ip", &["tuntap", "add", "dev", &name, "mode", "tap"])?;
        run(
            "ip",
            &["addr", "add", &format!("{}/30", host_ip), "dev", &name],
        )?;
        run("ip", &["link", "set", &name, "up"])?;

        Ok(TapDevice {
            name,
            host_ip,
            guest_ip,
            slot,
        })
    }

    /// Tear down the TAP device for a VM.
    pub fn release_tap(&self, slot: u32) -> Result<()> {
        let name = tap_name(slot);
        run("ip", &["link", "delete", &name])
    }

    /// List all TAP devices matching the `fc-tap-` prefix.
    pub fn list_tap_devices(&self) -> Result<Vec<String>> {
        let output = Command::new("ip").args(["tuntap", "show"]).output()?;

        if !output.status.success() {
            return Err(NetworkError::CommandFailed {
                cmd: "ip tuntap show".into(),
                stderr: String::from_utf8_lossy(&output.stderr).into(),
            });
        }

        let names = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let name = line.split(':').next()?.trim().to_string();
                name.starts_with("fc-tap-").then_some(name)
            })
            .collect();

        Ok(names)
    }
}

impl Default for NetworkManager {
    fn default() -> Self {
        Self::new()
    }
}

pub fn tap_name(slot: u32) -> String {
    format!("fc-tap-{slot}")
}

fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(cmd).args(args).output()?;
    if !output.status.success() {
        return Err(NetworkError::CommandFailed {
            cmd: format!("{} {}", cmd, args.join(" ")),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        });
    }
    Ok(())
}
