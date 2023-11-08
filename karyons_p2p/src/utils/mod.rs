mod version;

pub use version::{version_match, Version, VersionInt};

use std::net::IpAddr;

use karyons_net::Addr;

/// Check if two addresses belong to the same subnet.
pub fn subnet_match(addr: &Addr, other_addr: &Addr) -> bool {
    match (addr, other_addr) {
        (Addr::Ip(IpAddr::V4(ip)), Addr::Ip(IpAddr::V4(other_ip))) => {
            // XXX Consider moving this to a different location
            if other_ip.is_loopback() && ip.is_loopback() {
                return false;
            }
            ip.octets()[0..3] == other_ip.octets()[0..3]
        }
        _ => false,
    }
}
