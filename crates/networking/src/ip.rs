use std::net::Ipv4Addr;

/// Compute the host-side IP for slot N in the 172.16.0.0/16 space.
/// Each slot gets a /30: host = 172.16.N.1, guest = 172.16.N.2
pub fn host_ip(slot: u32) -> Ipv4Addr {
    let [a, b] = slot_octets(slot);
    Ipv4Addr::new(172, 16, a, b)
}

/// Compute the guest-side IP for slot N.
pub fn guest_ip(slot: u32) -> Ipv4Addr {
    let [a, b] = slot_octets(slot);
    Ipv4Addr::new(172, 16, a, b + 1)
}

/// Kernel boot args string that configures guest networking statically.
/// Format: ip=<guest>::<gateway>:<netmask>::<iface>:off
pub fn kernel_boot_args(slot: u32) -> String {
    let gip = guest_ip(slot);
    let hip = host_ip(slot);
    format!("ip={}::{}:255.255.255.252::eth0:off", gip, hip)
}

fn slot_octets(slot: u32) -> [u8; 2] {
    // slot N → 172.16.N.1 (host), 172.16.N.2 (guest), supports slots 1-254
    [slot as u8, 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_0_ips() {
        assert_eq!(host_ip(0), Ipv4Addr::new(172, 16, 0, 1));
        assert_eq!(guest_ip(0), Ipv4Addr::new(172, 16, 0, 2));
    }

    #[test]
    fn slot_1_ips() {
        assert_eq!(host_ip(1), Ipv4Addr::new(172, 16, 1, 1));
        assert_eq!(guest_ip(1), Ipv4Addr::new(172, 16, 1, 2));
    }

    #[test]
    fn kernel_args_slot_0() {
        let args = kernel_boot_args(0);
        assert!(args.contains("172.16.0.2"), "guest ip missing: {args}");
        assert!(args.contains("172.16.0.1"), "gateway missing: {args}");
    }
}
