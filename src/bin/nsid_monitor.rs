use std::os::{
    fd::{AsFd, AsRawFd, RawFd},
    raw::c_void,
};

use libc::{c_int, socklen_t};
use net_device_mapping::netns::NsId;
use socket2::{Protocol, Socket, Type};

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    println!("Opening socket");
    let socket = Socket::new(
        libc::AF_NETLINK.into(),
        Type::RAW,
        Some(Protocol::from(libc::NETLINK_ROUTE)),
    )?;
    let socket_fd: RawFd = socket.as_fd().as_raw_fd();

    // Set options
    unsafe {
        let socket = socket_fd;

        let listen_all: c_int = 1;
        let ret = libc::setsockopt(
            socket,
            libc::SOL_NETLINK,
            libc::NETLINK_LISTEN_ALL_NSID,
            &listen_all as *const _ as *const c_void,
            std::mem::size_of_val(&listen_all) as socklen_t,
        );
        if ret < 0 {
            Err(std::io::Error::last_os_error())?;
        }

        let group: c_int = libc::RTNLGRP_NSID as c_int;
        let ret = libc::setsockopt(
            socket,
            libc::SOL_NETLINK,
            libc::NETLINK_LISTEN_ALL_NSID,
            &group as *const _ as *const c_void,
            std::mem::size_of_val(&group) as socklen_t,
        );
        if ret < 0 {
            Err(std::io::Error::last_os_error())?;
        }
    }

    loop {
        let mut buffer = [0u8; 8192];

        let mut iovec = libc::iovec {
            iov_base: buffer.as_mut_ptr() as *mut _,
            iov_len: buffer.len(),
        };

        let mut socket_addr: libc::sockaddr_nl = unsafe { std::mem::zeroed() };

        const CONTROL_SIZE: usize = unsafe {libc::CMSG_SPACE(std::mem::size_of::<c_int>() as u32) as usize};
        let mut control_buf = [0u8; CONTROL_SIZE];

        let mut message = libc::msghdr {
            msg_name: &mut socket_addr as *mut _ as *mut _,
            msg_namelen: std::mem::size_of_val(&socket_addr) as u32,
            msg_iov: &mut iovec as *mut _ as *mut _,
            msg_iovlen: 1,
            msg_control: control_buf.as_mut_ptr() as *mut _,
            msg_controllen: control_buf.len(),
            msg_flags: 0,
        };

        println!("Trying to receive message");
        let len = unsafe { libc::recvmsg(socket_fd, &mut message as *mut _, 0) };
        if len < 0 {
            Err(std::io::Error::last_os_error())?;
        }
        println!("Received message of size {len}");

        let nsid = get_nsid_from_message(&message);

        println!("nsid = {nsid:?}");
    }

    Ok(())
}

fn get_nsid_from_message(message: &libc::msghdr) -> Option<NsId> {
    loop {
        let header = unsafe { libc::CMSG_FIRSTHDR(message as *const _) };
        if header.is_null() {
            break None;
        }
        unsafe {
            if (*header).cmsg_level == libc::SOL_NETLINK && (*header).cmsg_type == libc::NETLINK_LISTEN_ALL_NSID {
                return Some(*(libc::CMSG_DATA(header) as *const c_int) as _);
            }
        }
    }
}
