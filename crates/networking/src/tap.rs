use std::net::Ipv4Addr;
use std::process::Command;

use crate::{NetworkError, Result, ip};

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
    pub fn allocate_tap(&self, slot: u32) -> Result<TapDevice> {
        let name = tap_name(slot);
        let host_ip = ip::host_ip(slot);
        let guest_ip = ip::guest_ip(slot);

        run("ip", &["tuntap", "add", "dev", &name, "mode", "tap"])?;
        run("ip", &["addr", "add", &format!("{}/30", host_ip), "dev", &name])?;
        run("ip", &["link", "set", &name, "up"])?;

        Ok(TapDevice { name, host_ip, guest_ip, slot })
    }

    /// Tear down the TAP device for a VM.
    pub fn release_tap(&self, slot: u32) -> Result<()> {
        let name = tap_name(slot);
        run("ip", &["tuntap", "del", "dev", &name, "mode", "tap"])
    }

    /// List all TAP devices matching the `fc-tap-` prefix.
    pub fn list_tap_devices(&self) -> Result<Vec<String>> {
        let output = Command::new("ip")
            .args(["tuntap", "show"])
            .output()?;

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
