use std::net::{Ipv4Addr, Ipv6Addr};

use thiserror::Error;
use tokio::io::AsyncReadExt;

use crate::netns::INode;

pub struct PeerRef {
    name: String,
    netns: INode,
}

pub enum Kind {
    Ethernet,
    Wifi,
    Wwan,
    Ppp,
    Slip,
    Loopback,
    Veth { peer: PeerRef },
    Bridge { ports: Vec<PeerRef> },
    Bond { slaves: Vec<PeerRef> },
    Vlan { id: u16, parent: PeerRef },
    MacVlan { parent: PeerRef },
    IpVlan { parent: PeerRef },
    Vxlan { vni: u32 },
    Tun,
    Tap,
    Gre,
    Wireguard,

    Other(String),
}

pub type Mac = [u8; 6];
pub type Ipv4Mask = [u8; 4];
pub type Ipv6Mask = [u8; 16];

pub struct DeviceInfo {
    pub kind: Kind,
    pub name: String,
    pub mac_addr: Option<Mac>,
    pub ipv4_addrs: Vec<(Ipv4Addr, Ipv4Mask)>,
    pub ipv6_addrs: Vec<(Ipv6Addr, Ipv6Mask)>,
    pub netns: INode,
    pub is_up: bool,
    pub is_virtual: bool,
}

#[derive(Debug, Error)]
pub enum Error {}

impl DeviceInfo {
    pub async fn all() -> Result<Vec<DeviceInfo>, Error> {
        // Check devices in /sys/class/net
        // For virtual devices, check /sys/devices/virtual/net

        todo!()
    }
}
