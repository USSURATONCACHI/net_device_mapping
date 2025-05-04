#![allow(dead_code, unused)]

use std::{
    collections::HashMap,
    mem::needs_drop,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Output,
    sync::RwLock,
};

use net_device_mapping::{
    netns::{self, INode, NetworkNamespace, NsId, Pid},
    syscall_monitor::{self, EbpfEvent, EventType},
};
use thiserror::Error;
use tokio::{
    fs::metadata,
    sync::broadcast::{self, Receiver},
};

// #[derive(Clone, Debug)]
// pub struct ProcessState {
//     executable: PathBuf,
//     task_command: String,
//     netns: INode,
// }

#[derive(Debug, Error)]
pub enum Error {
    #[error("syscall monitor error - {0}")]
    Syscall(#[from] syscall_monitor::Error),
    #[error("io error - {0}")]
    Io(#[from] std::io::Error),
    #[error("Netns error - {0}")]
    Netns(#[from] netns::Error),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShallowNetns {
    // NETNSID. Network namespace can be assigned a small integer id.
    // This is also a way to uniquely identify network namespaces, but it can be not present.
    pub id: Option<NsId>,

    /// Network namespace can be bound to a specific file. This can serve as a user-defined name source.
    /// For example, `ip netns add <name>` creates a network namespace and binds it to `/run/netns/<name>` file.
    pub fs_path: Option<PathBuf>,

    /// Used as quick reference counting.
    pub pids_count: usize,
}

impl ShallowNetns {
    pub fn from_netns(netns: NetworkNamespace) -> (Self, INode, Vec<Pid>) {
        (
            Self {
                id: netns.id,
                fs_path: netns.fs_path,
                pids_count: netns.pids.len(),
            },
            netns.inode,
            netns.pids,
        )
    }
}

struct State {
    pub network_namespaces: HashMap<INode, ShallowNetns>,
    pub pids: HashMap<Pid, INode>,
}

impl State {
    pub async fn load() -> Result<Self, Error> {
        let mut pids = HashMap::new();

        let network_namespaces = NetworkNamespace::all()
            .await?
            .into_iter()
            .map(ShallowNetns::from_netns)
            .map(|(netns, inode, inode_pids)| {
                for pid in inode_pids {
                    pids.insert(pid, inode);
                }
                (inode, netns)
            })
            .collect();

        Ok(Self {
            network_namespaces,
            pids,
        })
    }

    pub fn add_pid(&mut self, pid: Pid, inode: INode) {
        self.pids.insert(pid, inode);
        self.network_namespaces
            .entry(inode)
            .and_modify(|netns| netns.pids_count += 1)
            .or_insert(ShallowNetns {
                id: None,
                fs_path: None,
                pids_count: 1,
            });
    }

    pub fn remove_pid(&mut self, pid: Pid) {
        if let Some(inode) = self.pids.remove(&pid) {
            let netns = self.network_namespaces.get_mut(&inode).unwrap();
            netns.pids_count -= 1;
            if netns.pids_count == 0 && netns.fs_path.is_none() {
                // Namespace is removed when its not bound to any path and have no PIDs
                self.network_namespaces.remove(&inode);
            }
        }
    }

    async fn handle_syscall_event(&mut self, event: EbpfEvent) -> std::io::Result<()> {
        // let netns_inode = get_process_netns(event.pid).await?;

        match event.kind {
            EventType::Fork => {
                let inode = get_process_netns(event.pid).await?;
                self.add_pid(event.pid, inode);
            }
            EventType::Exec => {}
            EventType::Exit => {
                self.remove_pid(event.pid);
            }
            EventType::Clone => todo!(),
            EventType::Unshare => todo!(),
            EventType::Setns => todo!(),
        }

        Ok(())
    }
}

pub async fn track_network_namespaces() -> Result<(), Error> {
    let (mut syscall_events, syscall_notifier) =
        net_device_mapping::syscall_monitor::monitor_syscalls()?;

    let mut state = State::load();
    tokio::spawn(syscall_notifier);

    // TODO: stopping oneshot channel.
    // TODO: Track nsfs entries from `/proc/self/mountinfo` to check for netns bound pathes change. +
    // TODO: Subscribe to rtnetlink netns ids for ID changes.
    // TODO: Periodically rescan procfs.

    todo!();
    // loop {
    //     match syscall_events.recv().await {
    //         Err(broadcast::error::RecvError::Closed) => break,
    //         Err(broadcast::error::RecvError::Lagged(_skipped)) => continue,

    //         Ok(event) => handle_syscall_event(&mut state, event).await?,
    //     }
    // }

    Ok(())
}

async fn get_process_netns(pid: Pid) -> std::io::Result<INode> {
    let path = Path::new("/proc")
        .join(pid.to_string())
        .join("ns")
        .join("net");

    let inode = metadata(path).await?.ino();

    Ok(inode)
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    todo!()
}
