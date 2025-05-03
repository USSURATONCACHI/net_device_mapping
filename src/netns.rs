use std::{
    collections::HashMap,
    ffi::OsString,
    fs::File,
    num::ParseIntError,
    os::{fd::{AsRawFd, RawFd}, unix::fs::MetadataExt},
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use glob::glob;
use mountinfo::{FsType, MountInfo};
use rtnetlink::{
    new_connection,
    packet_core::{NLM_F_REQUEST, NetlinkMessage, NetlinkPayload},
    packet_route::{
        AddressFamily, RouteNetlinkMessage,
        nsid::{NsidAttribute, NsidMessage},
    },
};
use thiserror::Error;
use tokio::fs::metadata;

type INode = u64;
type Pid = u32;

const PROCFS_GLOB_PATTERN: &'static str = "/proc/*/ns/net";

#[derive(Debug, Clone, PartialEq)]
pub struct NetworkNamespace {
    /// The way to differentiate namespaces on the system.
    /// Different namespaces will have different inodes, and same namespace will always have same inode.
    pub inode: INode,

    // NETNSID. Network namespace can be assigned a small integer id.
    // This is also a way to uniquely identify network namespaces, but it can be not present.
    pub id: Option<u32>,

    /// Network namespace can be bound to a specific file. This can serve as a user-defined name source.
    /// For example, `ip netns add <name>` creates a network namespace and binds it to `/run/netns/<name>` file.
    pub fs_path: Option<PathBuf>,

    /// List of all processes that are running in that namespace
    pub pids: Vec<Pid>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to get metadata for file {0} - {1}")]
    CouldntGetMetadata(PathBuf, std::io::Error),
    #[error("failed to read /proc/self/mountinfo {0}")]
    CouldntGetMountinfo(std::io::Error),
    #[error("io error - {0}")]
    IoError(#[from] std::io::Error),
    #[error("failed to query netns id - {0}")]
    IdQueryFailed(#[from] NetnsIdError),
}

impl NetworkNamespace {
    pub async fn all() -> Result<Vec<NetworkNamespace>, Error> {
        let files = glob(PROCFS_GLOB_PATTERN)
            .expect("Pattern should be correct")
            .filter_map(|file| file.ok())
            .filter_map(|file| parse_procfs_path_start(&file).map(|pid| (file, pid)).ok());

        // Map from netns inode, to list of PIDs in that inode.
        let mut inodes: HashMap<INode, NetworkNamespace> = HashMap::new();

        // Get all (possibly unnamed) network namespaces from processes list
        for (netns_link, pid) in files {
            let metadata = metadata(&netns_link)
                .await
                .map_err(|err| Error::CouldntGetMetadata(netns_link.clone(), err))?;
            let inode = metadata.ino();
            let pid = pid as Pid;

            inodes
                .entry(inode)
                .and_modify(|netns| netns.pids.push(pid))
                .or_insert(NetworkNamespace {
                    inode,
                    id: None,
                    fs_path: None,
                    pids: vec![pid],
                });
        }

        // Get all named namespaces from `/proc/self/mountinfo`.
        let mounts = MountInfo::new().map_err(|err| Error::CouldntGetMountinfo(err))?;
        for mount in mounts
            .mounting_points
            .into_iter()
            .filter(|x| x.fstype == FsType::Other("nsfs".to_owned()))
        {
            let path = mount.path;
            let metadata = metadata(&path).await?;
            let inode = metadata.ino();

            inodes
                .entry(inode)
                .and_modify(|netns| netns.fs_path = Some(path.clone()))
                .or_insert(NetworkNamespace {
                    inode,
                    id: None,
                    fs_path: Some(path),
                    pids: vec![],
                });
        }

        // Try to query ids for each namespace
        let (conn, mut handle, messages) = new_connection()?;
        let task = tokio::spawn(conn);

        for (_, netns) in &mut inodes {
            let Some(file) = netns.any_file() else {
                continue;
            };
            let Some(netnsid) = NetworkNamespace::id_by_filepath(&mut handle, file.as_path()).await?
            else {
                continue;
            };
            netns.id = Some(netnsid as u32);
        }

        drop(handle);
        drop(messages);
        task.await.unwrap();

        Ok(inodes.into_values().collect())
    }

    /// Returns an iterator of all all files that can be used to get a file descriptor of the inode.
    pub fn files(&self) -> impl Iterator<Item = PathBuf> {
        self.fs_path
            .iter()
            .cloned()
            .chain(self.pids.iter().map(|&pid| {
                Path::new("/proc")
                    .join(pid.to_string())
                    .join("ns")
                    .join("net")
            }))
    }

    /// Returns any file that can be used to get a file descriptor for that network namespace.
    pub fn any_file(&self) -> Option<PathBuf> {
        self.files().next()
    }

    pub async fn by_inode(handle: &mut rtnetlink::Handle, inode: INode) -> Result<Option<NetworkNamespace>, Error> {
        todo!()
    }

    pub async fn by_path(handle: &mut rtnetlink::Handle, path: PathBuf) -> Result<Option<NetworkNamespace>, Error> {
        let file = File::open(&path)
            .map_err(|err| Error::CouldntGetMetadata(path.clone(), err))?;

        Self::by_file(handle, &file).await
    }

    pub async fn by_file(handle: &mut rtnetlink::Handle, path: &File) -> Result<Option<NetworkNamespace>, Error> {
        todo!()
    }

    pub async fn by_id(handle: &mut rtnetlink::Handle, id: u32) -> Result<Option<NetworkNamespace>, Error> {
        todo!()
    }

    pub async fn subscribe_events() -> Result<(), ()> {
        todo!()
    }
}

#[derive(Debug, Error)]
pub enum NetnsIdError {
    #[error("could not open network namespace file - {0}")]
    CouldntOpenNetns(#[from] std::io::Error),
    #[error("failed to do rtnetlink request - {0}")]
    Rtnetlink(#[from] rtnetlink::Error),
}

impl NetworkNamespace {
    pub async fn id_by_filepath_own_connection(filepath: &Path) -> Result<Option<u32>, NetnsIdError> {
        let (conn, mut handle, messages) = new_connection()?;
        let task = tokio::spawn(conn);

        let result = Self::id_by_filepath(&mut handle, filepath).await;

        drop(handle);
        drop(messages);
        task.await.unwrap();

        result
    }

    pub async fn id_by_filepath(
        handle: &mut rtnetlink::Handle,
        filepath: &Path,
    ) -> Result<Option<u32>, NetnsIdError> {
        let file = File::open(filepath)?;

        Self::id_by_file(handle, &file).await
    }

    pub async fn id_by_file(
        handle: &mut rtnetlink::Handle,
        file: &File,
    ) -> Result<Option<u32>, NetnsIdError> {
        unsafe {
            Self::id_by_file_descriptor(handle, file.as_raw_fd()).await
        }
    }

    pub async unsafe fn id_by_file_descriptor(
        handle: &mut rtnetlink::Handle,
        fd: RawFd,
    ) -> Result<Option<u32>, NetnsIdError> {

        let mut message = NsidMessage::default();
        message.header.family = AddressFamily::Unspec;
        message
            .attributes
            .push(NsidAttribute::Fd(fd as u32));

        let mut request = NetlinkMessage::from(RouteNetlinkMessage::GetNsId(message));
        request.header.flags = NLM_F_REQUEST;

        let mut responses = handle.request(request)?;

        use futures::StreamExt;

        while let Some(msg) = responses.next().await {
            match msg.payload {
                NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewNsId(NsidMessage {
                    attributes,
                    ..
                })) => {
                    for attr in attributes {
                        match attr {
                            NsidAttribute::Id(id) | NsidAttribute::CurrentNsid(id) if id >= 0 => {
                                return Ok(Some(id as u32));
                            }
                            _ => {}
                        }
                    }
                }
                _other => {}
            }
        }

        Ok(None)
    }
}

#[derive(Debug, Error)]
enum ParseProcfsError {
    #[error("path is not absolute")]
    NotAbsolute,
    #[error("path does not start with root")]
    DoesntStartWithRoot,
    #[error("path does not start with `/proc/`")]
    NonProc,
    #[error("path does not contain a PID")]
    NoPid,
    #[error("PID OS string cannot be parsed")]
    ErrorneousOsPid(OsString),
    #[error("path has incorrect PID - '{0}' - {1}")]
    NotAPid(String, ParseIntError),
}

fn parse_procfs_path_start(path: &PathBuf) -> Result<u64, ParseProcfsError> {
    if !path.is_absolute() {
        return Err(ParseProcfsError::NotAbsolute);
    }
    let mut components = path.components();
    if !matches!(components.next(), Some(std::path::Component::RootDir)) {
        return Err(ParseProcfsError::DoesntStartWithRoot);
    }

    let proc: OsString = OsString::from_str("proc").unwrap();
    if !matches!(components.next(), Some(std::path::Component::Normal(x)) if x == proc) {
        return Err(ParseProcfsError::NonProc);
    }

    let Some(Component::Normal(pid)) = components.next() else {
        return Err(ParseProcfsError::NoPid);
    };
    let pid = match pid.to_str() {
        Some(x) => x,
        None => return Err(ParseProcfsError::ErrorneousOsPid(pid.to_owned())),
    };

    let pid = match pid.parse::<u64>() {
        Ok(pid) => pid,
        Err(err) => return Err(ParseProcfsError::NotAPid(pid.to_owned(), err)),
    };

    return Ok(pid);
}

