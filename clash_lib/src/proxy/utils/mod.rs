use std::{
    cmp::Ordering,
    fmt::Display,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

#[cfg(test)]
pub mod test_utils;

mod platform;

pub mod provider_helper;
mod proxy_connector;
mod socket_helpers;

use network_interface::{NetworkInterface, NetworkInterfaceConfig};
pub use proxy_connector::*;

use serde::{Deserialize, Serialize};
pub use socket_helpers::*;
use tracing::trace;

/// eth,en: Ethernet
/// wlan: Wireless
/// pdp_id: Cellular
// TODO: add it to configuartion
static INTERFACE_PRIORITY: [&str; 4] = ["eth", "en", "wlan", "pdp_ip"];

#[derive(Debug)]
pub struct OutboundInterface {
    pub name: String,
    #[allow(unused)]
    pub addr_v4: Option<Ipv4Addr>,
    #[allow(unused)]
    pub addr_v6: Option<Ipv6Addr>,
    #[allow(unused)]
    pub index: u32,
}

fn get_outbound_ip_from_interface(
    iface: &NetworkInterface,
) -> (Option<Ipv4Addr>, Option<Ipv6Addr>) {
    let mut v4 = None;
    let mut v6 = None;

    for addr in iface.addr.iter() {
        trace!("inspect interface address: {:?} on {}", addr, iface.name);

        if v4.is_some() && v6.is_some() {
            break;
        }

        match addr {
            network_interface::Addr::V4(addr) => {
                if !addr.ip.is_loopback()
                    && !addr.ip.is_link_local()
                    && !addr.ip.is_unspecified()
                {
                    v4 = Some(addr.ip);
                }
            }
            network_interface::Addr::V6(addr) => {
                if addr.ip.is_global() && !addr.ip.is_unspecified() {
                    v6 = Some(addr.ip);
                }
            }
        }
    }

    (v4, v6)
}

pub fn get_outbound_interface() -> Option<OutboundInterface> {
    let now = std::time::Instant::now();

    let mut all_outbounds = network_interface::NetworkInterface::show()
        .ok()?
        .into_iter()
        .filter_map(|iface| {
            let (addr_v4, addr_v6) = get_outbound_ip_from_interface(&iface);
            if !iface.name.contains("tun")
                && (addr_v4.is_some() || addr_v6.is_some())
            {
                Some(OutboundInterface {
                    name: iface.name,
                    addr_v4,
                    addr_v6,
                    index: iface.index,
                })
            } else {
                // pass interface created by tun, or lacks address
                None
            }
        })
        .collect::<Vec<_>>();

    all_outbounds.sort_by(|left, right| {
        match (left.addr_v6, right.addr_v6) {
            (Some(_), None) => return Ordering::Less,
            (None, Some(_)) => return Ordering::Greater,
            (Some(left), Some(right)) => {
                if left.is_unicast_global() && !right.is_unicast_global() {
                    return Ordering::Less;
                } else if !left.is_unicast_global() && right.is_unicast_global() {
                    return Ordering::Greater;
                }
            }
            _ => {}
        }
        let left = INTERFACE_PRIORITY
            .iter()
            .position(|x| left.name.contains(x))
            .unwrap_or(usize::MAX);
        let right = INTERFACE_PRIORITY
            .iter()
            .position(|x| right.name.contains(x))
            .unwrap_or(usize::MAX);

        left.cmp(&right)
    });

    trace!(
        "sorted outbound interfaces: {:?}, took: {}ms",
        all_outbounds,
        now.elapsed().as_millis()
    );

    all_outbounds.into_iter().next()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Interface {
    // TODO divide it into 2 or 4 cases
    // v4 single stack
    // v6 single stack
    // v4-v6 dual stack
    // v6-v4 dual stack
    IpAddr(Option<Ipv4Addr>, Option<Ipv6Addr>),
    Name(String),
}
impl From<OutboundInterface> for Interface {
    fn from(value: OutboundInterface) -> Self {
        if cfg!(not(target_os = "android")) {
            Self::Name(value.name)
        } else {
            Self::Name(value.name)
        }
    }
}
impl From<IpAddr> for Interface {
    fn from(value: IpAddr) -> Self {
        match value {
            IpAddr::V4(addr) => Self::IpAddr(Some(addr), None),
            IpAddr::V6(addr) => Self::IpAddr(None, Some(addr)),
        }
    }
}
impl From<&str> for Interface {
    fn from(value: &str) -> Self {
        Interface::Name(String::from(value))
    }
}

impl Display for Interface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Interface::IpAddr(v4, v6) => write!(f, "{v4:?} {v6:?}"),
            Interface::Name(name) => write!(f, "{}", name),
        }
    }
}

impl Interface {
    pub fn into_iface_name(self) -> Option<String> {
        match self {
            Interface::IpAddr(..) => None,
            Interface::Name(name) => Some(name),
        }
    }
}
