use std::{
    collections::{HashMap, HashSet},
    os::{
        fd::{AsFd, AsRawFd},
        unix::fs::MetadataExt,
    },
    path::{Path, PathBuf},
    str::FromStr,
};

use futures::StreamExt;
use itertools::Itertools;
use mountinfo::MountInfo;
use thiserror::Error;
use tokio::{
    fs::metadata,
    sync::broadcast::{Receiver, Sender},
};
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

use crate::{
    mount_monitor::{FsType, MountChange, MountPoint},
    netns::{INode, NetworkNamespace, NsId, Pid, PidsIterator},
    nsid_monitor::NetnsIdEvent,
    syscall_monitor::EbpfEvent,
};

pub type StateRequest = ();
pub type StateResponse = Vec<NetworkNamespace>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error - {0}")]
    Io(#[from] std::io::Error),
    #[error("netns error - {0}")]
    Netns(#[from] crate::netns::Error),
}

#[derive(Debug, Clone)]
enum Event {
    NetnsIdEvent(NetnsIdEvent),
    MountChange(MountChange),
    Syscall(EbpfEvent),
    StateRequested(StateRequest),
}

pub fn monitor_network_namespaces(
    nsid_events: Receiver<NetnsIdEvent>,
    mount_events: Receiver<MountChange>,
    syscalls: Receiver<EbpfEvent>,
) -> Result<
    (
        Sender<StateRequest>,
        Receiver<StateResponse>,
        impl Send + Future<Output = Result<(), Error>>,
    ),
    Error,
> {
    // Create a channel for receiving data from here
    let (state_request_tx, state_request_rx) = tokio::sync::broadcast::channel(1024);
    let (state_response_tx, state_response_rx) = tokio::sync::broadcast::channel(1024);

    let events = {
        // Combine all streams into a single one
        let state_requests = BroadcastStream::new(state_request_rx)
            .filter_map(async |x| x.ok())
            .map(|()| Event::StateRequested(()));

        let nsid_events = BroadcastStream::new(nsid_events)
            .filter_map(async |x| x.ok())
            .map(|netns_event| Event::NetnsIdEvent(netns_event));

        let mount_events = BroadcastStream::new(mount_events)
            .filter_map(async |x| x.ok())
            .filter(|mount_change| {
                let target_fstype = FsType::Other("nsfs".to_owned());
                let result = match mount_change {
                    MountChange::Added(_uuid, mount_point) => mount_point.fstype == target_fstype,
                    MountChange::Removed(_uuid) => true,
                    MountChange::Modified(_uuid, mount_point) => {
                        mount_point.fstype == target_fstype
                    }
                };
                async move { result }
            })
            .map(|netns_event| Event::MountChange(netns_event));

        let syscalls = BroadcastStream::new(syscalls)
            .filter_map(async |x| x.ok())
            .map(|netns_event| Event::Syscall(netns_event));

        let events = nsid_events;
        let events = tokio_stream::StreamExt::merge(events, mount_events);
        let events = tokio_stream::StreamExt::merge(events, syscalls);
        let events = tokio_stream::StreamExt::merge(events, state_requests);
        events
    };

    // Connection to query IDs for network namespaces
    let (conn, mut handle, messages) = rtnetlink::new_connection()?;
    drop(messages);
    let rtnetlink_task: tokio::task::JoinHandle<()> = tokio::spawn(conn);

    // Run the future
    let fut = async move {
        let mut ev = std::pin::pin!(events);

        let mut state = State::new().await?;
        let mut mount_state = MountState::default();

        'main: loop {
            tokio::select! {
                _ = state_response_tx.closed() => break 'main,

                event = ev.next() => {
                    if let Some(event) = event {
                        let should_quit = process_event(&mut state, &mut mount_state, &mut handle, &state_response_tx, event).await?;
                        if should_quit {
                            break 'main;
                        }
                    }
                }

            }
        }

        drop(handle); // Avoid deadlock.
        rtnetlink_task.await.unwrap();
        Ok(())
    };

    Ok((state_request_tx, state_response_rx, fut))
}

async fn process_event(
    state: &mut State,
    mount_state: &mut MountState,
    handle: &mut rtnetlink::Handle,
    state_response_tx: &Sender<StateResponse>,

    event: Event,
) -> Result<bool, Error> {
    match event {
        // ==== Network namespace id change ====
        Event::NetnsIdEvent(netns_id_event) => match netns_id_event {
            NetnsIdEvent::Added(id) => {
                if let Some(inode) = find_netns_id_addition(&state, handle, id).await? {
                    state.ensure_namespace_mut(inode).id = Some(id);
                } else {
                    use std::io::Write;
                    writeln!(std::io::stdout().lock(), "WARN: Failed to find namespace for assigned ID {id}. Might be bad.").unwrap();
                }
            }
            NetnsIdEvent::Removed(id) => {
                // Losing an ID means that namespace is removed.
                if let Some((inode, _)) = state.namespace_by_id(id) {
                    state.remove_namespace(inode);
                }
            }
        },

        // ==== NSFS partition was mounted, unmounted, or remounted ====
        Event::MountChange(mount_change) => {
            match &mount_change {
                MountChange::Added(_uuid, mount_point) => {
                    // Add the bound path
                    if let Ok(metadata) = metadata(&mount_point.path).await {
                        state
                            .ensure_namespace_mut(metadata.ino())
                            .fs_path
                            .insert(mount_point.path.clone());
                    }
                }
                MountChange::Removed(uuid) => {
                    let removed = mount_state
                        .get_path(*uuid)
                        .map(|path| (path, state.namespace_by_path(path)));

                    if let Some((path, Some((inode, namespace)))) = removed {
                        namespace.fs_path.remove(path);
                        let pathes_count = namespace.fs_path.len();

                        // No PIDs and no bound path = namespace deleted.
                        if pathes_count == 0 && state.does_namespace_has_pids(&inode) {
                            state.remove_namespace(inode);
                        }
                    }
                }
                MountChange::Modified(uuid, mount_point) => {
                    // Update the filepath it is bound to.
                    let removed = mount_state
                        .get_path(*uuid)
                        .map(|path| (path, state.namespace_by_path(path)));

                    if let Some((old_path, Some((_inode, namespace)))) = removed {
                        namespace.fs_path.remove(old_path);
                        namespace.fs_path.insert(mount_point.path.clone());
                    }
                }
            }
            mount_state.on_event(mount_change);
        }

        // ==== Some process did one of syscalls we are interested in ====
        Event::Syscall(ebpf_event) => {
            match ebpf_event.kind {
                crate::syscall_monitor::EventType::Fork | // Check process netns, and add PID to correct namespace.
                crate::syscall_monitor::EventType::Clone |
                crate::syscall_monitor::EventType::Unshare | // Check process netns, it may have changed (unshare with `CLONE_NEWNET` or setns with specific fd).
                crate::syscall_monitor::EventType::Setns => {
                    if let Ok(meta) = metadata(process_netns_path(ebpf_event.pid)).await {
                        state.ensure_namespace_mut(meta.ino());
                        state.pids.insert(ebpf_event.pid, meta.ino());
                    }
                },
                crate::syscall_monitor::EventType::Exit => {
                    state.pids.remove(&ebpf_event.pid);
                },
                crate::syscall_monitor::EventType::Exec => {}, // Does not do anything with namespaces
            }
        }

        // ==== User requested current state ====
        Event::StateRequested(()) => {
            if state_response_tx.send(state.current_state()).is_err() {
                return Ok(true);
            }
        }
    };
    Ok(false)
}

async fn find_netns_id_addition(
    state: &State,
    handle: &mut rtnetlink::Handle,
    id: NsId,
) -> std::io::Result<Option<INode>> {
    // 1. Happy path: rescan existing network namespaces
    for (inode, filepath) in state.namespace_files() {
        let Ok(file) = tokio::fs::File::open(filepath).await else {
            continue;
        };
        let netns_id_result = unsafe {
            NetworkNamespace::id_by_file_descriptor(handle, file.as_fd().as_raw_fd()).await
        };
        let Ok(Some(current_netns_id)) = netns_id_result else {
            continue;
        };

        if current_netns_id == id {
            return Ok(Some(inode));
        }
    }

    // 2. Less happy path: rescan all `/run/netns/` entries.
    let mounts = MountInfo::new()?
        .mounting_points
        .into_iter()
        .filter(|mount| matches!(&mount.fstype, mountinfo::FsType::Other(other) if other == "nsfs"))
        .map(|mount| mount.path)
        .sorted()
        .dedup();

    for filepath in mounts {
        let Ok(file) = tokio::fs::File::open(filepath).await else {
            continue;
        };
        let Ok(meta) = file.metadata().await else {
            continue;
        };
        let netns_id_result = unsafe {
            NetworkNamespace::id_by_file_descriptor(handle, file.as_fd().as_raw_fd()).await
        };
        let Ok(Some(current_netns_id)) = netns_id_result else {
            continue;
        };

        if current_netns_id == id {
            return Ok(Some(meta.ino()));
        }
    }

    // 3. Really unhappy path: rescan all processes.
    let mut pids = PidsIterator::new();
    loop {
        let (filepath, _pid, inode) = match pids.next().await {
            Ok(Some(x)) => x,
            Ok(None) => break,
            Err(_) => continue,
        };
        let Ok(Some(current_netns_id)) = NetworkNamespace::id_by_path(handle, &filepath).await
        else {
            continue;
        };
        if current_netns_id == id {
            return Ok(Some(inode));
        }
    }

    Ok(None)
}

fn process_netns_path(pid: Pid) -> PathBuf {
    PathBuf::from_str("/proc")
        .unwrap()
        .join(pid.to_string())
        .join("ns")
        .join("net")
}

/// Only some data from `NetworkNamespace` for optimized storage.
struct ShallowNamespace {
    /// NETNSID. Network namespace can be assigned a small integer id.
    /// This is also a way to uniquely identify network namespaces, but it can be not present.
    pub id: Option<NsId>,

    /// Network namespace can be bound to a specific file. This can serve as a user-defined name source.
    /// For example, `ip netns add <name>` creates a network namespace and binds it to `/run/netns/<name>` file.
    pub fs_path: HashSet<PathBuf>,
}

struct State {
    /// INodes are the way to differentiate namespaces on the system.
    /// Different namespaces will have different inodes, and same namespace will always have same inode.
    pub namespaces: HashMap<INode, ShallowNamespace>,

    /// Each process (`/proc/*/task/*/`, not group) is in exactly one network namespace.
    pub pids: HashMap<Pid, INode>,
}

impl State {
    pub async fn new() -> Result<Self, Error> {
        let iter = NetworkNamespace::all().await?.into_iter().map(|netns| {
            (
                netns.inode,
                ShallowNamespace {
                    id: netns.id,
                    fs_path: netns.fs_path,
                },
                netns.pids,
            )
        });

        let mut namespaces = HashMap::new();
        let mut pids = HashMap::new();

        for (inode, netns, netns_pids) in iter {
            namespaces.insert(inode, netns);

            for pid in netns_pids {
                pids.insert(pid, inode);
            }
        }

        Ok(Self { namespaces, pids })
    }

    pub fn current_state(&self) -> Vec<NetworkNamespace> {
        // Invert the hashmap.
        let mut pids_per_inode = self.pids.iter().map(|(&k, &v)| (v, k)).into_group_map();

        // Reconstruct the state.
        self.namespaces
            .iter()
            .map(|(&inode, netns)| NetworkNamespace {
                inode,
                id: netns.id.clone(),
                fs_path: netns.fs_path.clone(),
                pids: pids_per_inode.remove(&inode).unwrap_or_else(|| Vec::new()),
            })
            .collect()
    }

    pub fn ensure_namespace_mut(&mut self, inode: INode) -> &mut ShallowNamespace {
        if !self.namespaces.contains_key(&inode) {
            self.namespaces.insert(
                inode,
                ShallowNamespace {
                    id: None,
                    fs_path: HashSet::new(),
                },
            );
        }

        self.namespaces.get_mut(&inode).unwrap()
    }
    pub fn namespace_mut(&mut self, inode: INode) -> Option<&mut ShallowNamespace> {
        self.namespaces.get_mut(&inode)
    }
    pub fn namespace_by_id(&mut self, id: NsId) -> Option<(INode, &mut ShallowNamespace)> {
        self.namespaces
            .iter_mut()
            .find(|(_, netns)| netns.id == Some(id))
            .map(|(&k, v)| (k, v))
    }
    pub fn namespace_by_path(&mut self, path: &Path) -> Option<(INode, &mut ShallowNamespace)> {
        self.namespaces
            .iter_mut()
            .find(|(_, netns)| netns.fs_path.contains(path))
            .map(|(&k, v)| (k, v))
    }

    pub fn add_namespace(&mut self, netns: NetworkNamespace) -> Option<NetworkNamespace> {
        if self.namespaces.contains_key(&netns.inode) {
            Some(netns)
        } else {
            for pid in netns.pids {
                self.pids.insert(pid, netns.inode);
            }
            self.namespaces.insert(
                netns.inode,
                ShallowNamespace {
                    id: netns.id,
                    fs_path: netns.fs_path,
                },
            );
            None
        }
    }

    pub fn remove_namespace(&mut self, inode: INode) -> bool {
        if self.namespaces.remove(&inode).is_some() {
            self.pids = self
                .pids
                .iter()
                .map(|(&k, &v)| (k, v))
                .filter(|(_k, v)| v != &inode)
                .collect();

            true
        } else {
            false
        }
    }

    pub fn does_namespace_has_pids(&self, namespace: &INode) -> bool {
        self.pids.iter().any(|(_pid, inode)| inode == namespace)
    }

    pub fn namespace_any_file(&self, namespace: INode) -> Option<PathBuf> {
        self.namespaces
            .get(&namespace)
            .map(|netns| netns.fs_path.iter().next())
            .flatten()
            .cloned()
            .or_else(|| {
                self.pids
                    .iter()
                    .filter_map(|(pid, inode)| {
                        (*inode == namespace).then(|| process_netns_path(*pid))
                    })
                    .next()
            })
    }

    pub fn namespace_files(&self) -> impl Iterator<Item = (INode, PathBuf)> {
        self.namespaces
            .iter()
            .filter_map(|(inode, _)| self.namespace_any_file(*inode).map(|x| (*inode, x)))
    }
}

#[derive(Debug, Clone, Default)]
struct MountState {
    mounts: HashMap<Uuid, MountPoint>,
}
impl MountState {
    pub fn on_event(&mut self, event: MountChange) {
        match event {
            MountChange::Added(uuid, mount_point) => self.mounts.insert(uuid, mount_point),
            MountChange::Removed(uuid) => self.mounts.remove(&uuid),
            MountChange::Modified(uuid, mount_point) => self.mounts.insert(uuid, mount_point),
        };
    }

    pub fn has_path(&self, path: &Path) -> bool {
        self.mounts
            .iter()
            .any(|(_, mountpoint)| mountpoint.path == path)
    }

    pub fn get_path(&self, uuid: Uuid) -> Option<&PathBuf> {
        self.mounts.get(&uuid).map(|m| &m.path)
    }

    pub fn all_paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.mounts.values().map(|m| &m.path).sorted().dedup()
    }
}
