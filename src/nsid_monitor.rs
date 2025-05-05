use thiserror::Error;

pub type NsId = i32;

#[derive(Debug)]
pub enum NetnsIdEvent {
    Added(NsId),
    Removed(NsId),
}

#[derive(Debug, Error)]
pub enum MonitorError {
    #[error("rtnetlink failure - {0}")]
    Netlink(#[from] rtnetlink::Error),
    #[error("io error - {0}")]
    Io(#[from] std::io::Error),
}

// /// Returns a Receiver for NetnsIdEvent and a Future that drives the monitor loop.
// pub fn monitor_netns_ids() -> Result<
//     (
//         Receiver<NetnsIdEvent>,
//         impl Future<Output = Result<(), rtnetlink::Error>>,
//     ),
//     MonitorError,
// > {
//     // 1. Establish connection
//     let (conn, mut handle, mut messages) = rtnetlink::new_connection()?;
//     // Spawn the background task to run the connection
//     tokio::spawn(conn);

//     // 2. Subscribe to NSID group
//     handle.link().property_add(index)
//     handle.socket_mut().add_membership(RTNLGRP_NSID as u32)?;

//     // 3. Prepare channel for events
//     let (tx, rx) = channel(64);

//     // 4. Monitor loop
//     let monitor_fut = async move {
//         while let Some((message, _addr)) = messages.next().await {
//             if let RouteNetlinkMessage::NewNsId(n) = message.payload {
//                 if let Some(id) = n.nlas.iter().find_map(|attr| {
//                     if let netlink_packet_route::rtnl::nsid::NsidNla::Id(x) = attr {
//                         Some(*x)
//                     } else {
//                         None
//                     }
//                 }) {
//                     tx.send(NetnsIdEvent::Added(id)).await.map_err(|_| MonitorError::Send)?;
//                 }
//             } else if let RouteNetlinkMessage::DelNsId(n) = message.payload {
//                 if let Some(id) = n.nlas.iter().find_map(|attr| {
//                     if let netlink_packet_route::rtnl::nsid::NsidNla::Id(x) = attr {
//                         Some(*x)
//                     } else {
//                         None
//                     }
//                 }) {
//                     tx.send(NetnsIdEvent::Removed(id)).await.map_err(|_| MonitorError::Send)?;
//                 }
//             }
//         }
//         Ok(())
//     };

//     Ok((rx, monitor_fut))
// }
