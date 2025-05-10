use std::{
    any::Any,
    net::{Ipv4Addr, Ipv6Addr},
    os::fd::AsRawFd,
    path::PathBuf,
};

use futures::TryStreamExt;
use libc::CLONE_NEWNET;
use rtnetlink::packet_route::link::LinkMessage;
use thiserror::Error;
use tokio::task::LocalSet;

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

type ThreadError = Box<dyn Any + Send + 'static>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error - {0}")]
    Io(#[from] std::io::Error),
}

impl DeviceInfo {
    pub async fn all(
        _network_namespaces_files: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Vec<DeviceInfo>, Error> {
        // Check devices in /sys/class/net
        // For virtual devices, check /sys/devices/virtual/net

        todo!()

        // TODO: network device packet sniffer
    }
}

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("could not open network namespace file - {0}")]
    CoulndtOpenNetns(std::io::Error),

    #[error("thread died")]
    ThreadDied(ThreadError),

    #[error("failed to create tokio runtime - {0}")]
    TokioRuntime(std::io::Error),

    #[error("failed to open rtnetlink connection - {0}")]
    NetlinkConnection(std::io::Error),

    #[error("rtnetlink receiving error - {0}")]
    RtnetnlinkRecvErrror(#[from] rtnetlink::Error),
}

/// Moves to a certain network namespace, then uses rtnetlink to get all network devices
pub async fn query_netns_links(netns_filepath: PathBuf) -> Result<Vec<LinkMessage>, QueryError> {
    // 1. Open network namespace file (we need file descriptor)
    let handle = async_thread::spawn(move || -> Result<Vec<LinkMessage>, QueryError> {
        {
            let netns_file =
                std::fs::File::open(netns_filepath).map_err(QueryError::CoulndtOpenNetns)?;

            // 2. Move current thread to that network namespace
            set_netns(&netns_file).map_err(QueryError::CoulndtOpenNetns)?;
            let _ = netns_file; // we can close the file now 
        }

        // 3. Create async context from current thread.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(QueryError::TokioRuntime)?;
        let local_set = LocalSet::new();

        let local_set_ref = &local_set;
        let binding = async move || -> Result<Vec<LinkMessage>, QueryError> {
            // 4. Open rtnetlink socket
            let (conn, handle, _) =
                rtnetlink::new_connection().map_err(QueryError::NetlinkConnection)?;

            let conn_handle = local_set_ref.spawn_local(conn);

            let mut stream = handle.link().get().execute();
            let mut links = Vec::new();

            // 5. Receive all the messages
            while let Some(item) = TryStreamExt::try_next(&mut stream).await? {
                links.push(item);
            }

            let _ = handle;
            conn_handle.abort();

            Ok(links)
        };
        let links = local_set.block_on(&runtime, binding())?;

        Ok(links)
    });

    handle.join().await.map_err(QueryError::ThreadDied)?
}

fn set_netns(fd: &std::fs::File) -> std::io::Result<()> {
    unsafe {
        if libc::setns(fd.as_raw_fd(), CLONE_NEWNET) != 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
