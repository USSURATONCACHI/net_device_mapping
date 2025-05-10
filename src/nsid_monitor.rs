use futures::StreamExt;
use libc::RTNLGRP_NSID;
use rtnetlink::{
    packet_core::{NetlinkMessage, NetlinkPayload},
    packet_route::{
        RouteNetlinkMessage,
        nsid::{NsidAttribute, NsidMessage},
    },
    sys::{AsyncSocket, SocketAddr},
};
use thiserror::Error;
use tokio::sync::broadcast::Receiver;

use crate::netns::NsId;

#[derive(Debug, Clone, Copy)]
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

/// Returns a Receiver for NetnsIdEvent and a Future that drives the monitor loop.
pub fn monitor_netns_ids() -> Result<
    (
        Receiver<NetnsIdEvent>,
        impl Send + Future<Output = Result<(), rtnetlink::Error>>,
    ),
    MonitorError,
> {
    let (mut conn, handle, mut messages) = rtnetlink::new_connection()?;
    drop(handle);

    // Subscribe to NSID group
    {
        let socket = conn
            .socket_mut() // &mut TokioSocket
            .socket_mut(); // &mut netlink_sys::socket::Socket

        socket.bind(&SocketAddr::new(0, 0))?;
        socket.add_membership(RTNLGRP_NSID as u32)?;
    }
    let fut_handle = tokio::spawn(conn);

    let (send, recv) = tokio::sync::broadcast::channel(1024);

    // Receive events
    let monitor_fut = async move {
        'main: loop {
            tokio::select! {
                message = messages.next() => {
                    let Some(message) = message else {
                        break 'main;
                    };
                    let (message, _addr): (NetlinkMessage<RouteNetlinkMessage>, SocketAddr) = message;

                    let event = match message.payload {
                        NetlinkPayload::InnerMessage(inner) => match inner {
                            RouteNetlinkMessage::NewNsId(NsidMessage { attributes, .. }) => {
                                extract_nsid_from_attrs(attributes)
                                    .map(|x| NetnsIdEvent::Added(x))
                            }
                            RouteNetlinkMessage::DelNsId(NsidMessage { attributes, .. }) => {
                                extract_nsid_from_attrs(attributes)
                                    .map(|x| NetnsIdEvent::Removed(x))
                            }
                            _ => continue,
                        }
                        _other => continue,
                    };

                    if let Some(event) = event {
                        if send.send(event).is_err() {
                            break 'main;
                        }
                    }

                }

                _ = send.closed() => break 'main,
            }
        }
        drop(messages);
        fut_handle.abort();
        Ok(())
    };

    Ok((recv, monitor_fut))
}

fn extract_nsid_from_attrs(attrs: impl IntoIterator<Item = NsidAttribute>) -> Option<NsId> {
    for attr in attrs.into_iter() {
        match attr {
            NsidAttribute::Id(id) => return Some(id as NsId),
            _ => {}
        }
    }
    None
}
