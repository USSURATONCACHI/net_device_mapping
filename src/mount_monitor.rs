use std::{collections::{HashMap, HashSet}, path::PathBuf, str::FromStr};

use mountinfo::MountInfo;
use thiserror::Error;
use tokio::sync::broadcast::{Receiver, Sender, error::SendError};
use uuid::Uuid;

use crate::util::SendMonitor;

/// Exact copy of `mountinfo::ReadWrite`, but implements `Clone` and other traits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReadWrite {
    ReadOnly,
    ReadWrite,
}
impl From<mountinfo::ReadWrite> for ReadWrite {
    fn from(value: mountinfo::ReadWrite) -> Self {
        match value {
            mountinfo::ReadWrite::ReadOnly => ReadWrite::ReadOnly,
            mountinfo::ReadWrite::ReadWrite => ReadWrite::ReadWrite,
        }
    }
}
impl Into<mountinfo::ReadWrite> for ReadWrite {
    fn into(self) -> mountinfo::ReadWrite {
        match self {
            ReadWrite::ReadOnly => mountinfo::ReadWrite::ReadOnly,
            ReadWrite::ReadWrite => mountinfo::ReadWrite::ReadWrite,
        }
    }
}

/// Exact copy of `mountinfo::MountOptions`, but implements `Clone` and other traits.
#[derive(Debug, Clone, PartialEq)]
pub struct MountOptions {
    /// If it was mounted as read-only or read-write.
    pub read_write: ReadWrite,
    /// Additional options, not currently parsed by this library.
    pub others: Vec<String>,
}
impl From<mountinfo::MountOptions> for MountOptions {
    fn from(value: mountinfo::MountOptions) -> Self {
        MountOptions {
            read_write: value.read_write.into(),
            others: value.others,
        }
    }
}
impl Into<mountinfo::MountOptions> for MountOptions {
    fn into(self) -> mountinfo::MountOptions {
        mountinfo::MountOptions {
            read_write: self.read_write.into(),
            others: self.others,
        }
    }
}

/// Exact copy of `mountinfo::FsType`, but implements `Clone` and other traits.
#[derive(Debug, Clone, PartialEq)]
pub enum FsType {
    /// procfs filesystem. Pseudo filesystem that exposes the kernel's process table.
    /// Usually mounted at /proc.
    Proc,
    /// overlayfs filesystem. A filesystem that combines multiple lower filesystems into a single directory.
    Overlay,
    /// tmpfs filesystem. A filesystem that provides a temporary file system stored in volatile memory.
    Tmpfs,
    /// sysfs filesystem. A filesystem that provides access to the kernel's internal device tree.
    Sysfs,
    /// btrfs filesystem. A filesystem that provides a hierarchical data structure for storing data in a compressed fashion.
    Btrfs,
    /// ext2 filesystem. A filesystem that provides a file system that is optimized for storing data on a local disk.
    Ext2,
    /// ext3 filesystem. A filesystem that provides a file system that is optimized for storing data on a local disk.
    Ext3,
    /// ext4 filesystem. A filesystem that provides a file system that is optimized for storing data on a local disk.
    Ext4,
    /// devtmpfs filesystem.
    Devtmpfs,
    /// Other filesystems.
    Other(String),
}
impl From<mountinfo::FsType> for FsType {
    fn from(value: mountinfo::FsType) -> Self {
        match value {
            mountinfo::FsType::Proc => FsType::Proc,
            mountinfo::FsType::Overlay => FsType::Overlay,
            mountinfo::FsType::Tmpfs => FsType::Tmpfs,
            mountinfo::FsType::Sysfs => FsType::Sysfs,
            mountinfo::FsType::Btrfs => FsType::Btrfs,
            mountinfo::FsType::Ext2 => FsType::Ext2,
            mountinfo::FsType::Ext3 => FsType::Ext3,
            mountinfo::FsType::Ext4 => FsType::Ext4,
            mountinfo::FsType::Devtmpfs => FsType::Devtmpfs,
            mountinfo::FsType::Other(x) => FsType::Other(x),
        }
    }
}
impl Into<mountinfo::FsType> for FsType {
    fn into(self) -> mountinfo::FsType {
        match self {
            FsType::Proc => mountinfo::FsType::Proc,
            FsType::Overlay => mountinfo::FsType::Overlay,
            FsType::Tmpfs => mountinfo::FsType::Tmpfs,
            FsType::Sysfs => mountinfo::FsType::Sysfs,
            FsType::Btrfs => mountinfo::FsType::Btrfs,
            FsType::Ext2 => mountinfo::FsType::Ext2,
            FsType::Ext3 => mountinfo::FsType::Ext3,
            FsType::Ext4 => mountinfo::FsType::Ext4,
            FsType::Devtmpfs => mountinfo::FsType::Devtmpfs,
            FsType::Other(x) => mountinfo::FsType::Other(x),
        }
    }
}

/// Exact copy of `mountinfo::MountPoint`, but implements `Clone`.
#[derive(Debug, Clone, PartialEq)]
pub struct MountPoint {
    /// The id of the mount point. It is unique for each mount point,
    /// but can be resused afer a call to the umount syscall.
    pub id: Option<u32>,
    /// The id of the parent mount.
    pub parent_id: Option<u32>,
    /// The path to the directory that acts as the root for this mount point.
    pub root: Option<PathBuf>,
    // Filesystem-specific information
    pub what: String,
    /// The mount point directory relative to the root.
    pub path: PathBuf,
    /// The filesystem type.
    pub fstype: FsType,
    /// Some additional mount options
    pub options: MountOptions,
}
impl From<mountinfo::MountPoint> for MountPoint {
    fn from(value: mountinfo::MountPoint) -> Self {
        Self {
            id: value.id,
            parent_id: value.parent_id,
            root: value.root,
            what: value.what,
            path: value.path,
            fstype: value.fstype.into(),
            options: value.options.into(),
        }
    }
}
impl Into<mountinfo::MountPoint> for MountPoint {
    fn into(self) -> mountinfo::MountPoint {
        mountinfo::MountPoint {
            id: self.id,
            parent_id: self.parent_id,
            root: self.root,
            what: self.what,
            path: self.path,
            fstype: self.fstype.into(),
            options: self.options.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum MountChange {
    Added(Uuid, MountPoint),
    Removed(Uuid),
    Modified(Uuid, MountPoint),
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error - {0}")]
    Io(#[from] std::io::Error),
    #[error("send error - {0}")]
    Send(#[from] SendError<MountChange>),
    #[error("libmount monitor has no file descriptor - {0}")]
    NoMonitorFd(std::io::Error),
}

struct State {
    /// State of `/proc/self/mountinfo`.
    ///
    /// UUID v4 is only used to track mountpoints in context of this state, since mountpoint itself does not have any globally-unique field.
    pub mountinfo: HashMap<Uuid, MountPoint>,
}

impl State {
    pub fn new() -> std::io::Result<Self> {
        let mountpoints = MountInfo::new()?;

        Ok(Self {
            mountinfo: mountpoints
                .mounting_points
                .into_iter()
                .map(|mount| (Uuid::new_v4(), mount.into()))
                .collect(),
        })
    }

    /// Returns `false` if sending an event failed (sender is closed). `true` otherwise
    pub fn update_mountinfo(
        &mut self,
        send_events: &mut Sender<MountChange>,
    ) -> std::io::Result<bool> {
        let rescanned: Vec<MountPoint> = MountInfo::new()?
            .mounting_points
            .into_iter()
            .map(MountPoint::from)
            .collect();

        // 2. Build look-ups of the *old* state:
        let mut old_by_id:   HashMap<u32, Uuid>    = HashMap::new();
        let mut old_by_path: HashMap<PathBuf, Uuid> = HashMap::new();
        for (uuid, mp) in &self.mountinfo {
            if let Some(id) = mp.id {
                old_by_id.insert(id, *uuid);
            } else {
                old_by_path.insert(mp.path.clone(), *uuid);
            }
        }

        // This will become our new state:
        let mut new_map: HashMap<Uuid, MountPoint> = HashMap::new();
        // Track which old UUIDs we’ve seen again:
        let mut seen_old: HashSet<Uuid> = HashSet::new();

        // 3. For each newly scanned mountpoint, decide Added/Modified/Unchanged:
        for mp in rescanned {
            // Try match by kernel mount-ID first:
            if let Some(id) = mp.id {
                if let Some(&uuid) = old_by_id.get(&id) {
                    let old_mp = &self.mountinfo[&uuid];
                    // Did the mountpoint move paths?  Treat as remove + add
                    if mp.path != old_mp.path {
                        // Removal of the old
                        if send_events.send(MountChange::Removed(uuid)).is_err() {
                            return Ok(false);
                        }
                        // Addition of the “new” mount
                        let new_uuid = Uuid::new_v4();
                        if send_events.send(MountChange::Added(new_uuid, mp.clone())).is_err() {
                            return Ok(false);
                        }
                        new_map.insert(new_uuid, mp);
                    }
                    // Same path but other metadata changed?
                    else if &mp != old_mp {
                        if send_events.send(MountChange::Modified(uuid, mp.clone())).is_err() {
                            return Ok(false);
                        }
                        new_map.insert(uuid, mp);
                    }
                    // Unchanged
                    else {
                        new_map.insert(uuid, mp);
                    }
                    seen_old.insert(uuid);
                    continue;
                }
            }

            // Fallback: match by path for mounts without a usable ID
            if let Some(&uuid) = old_by_path.get(&mp.path) {
                let old_mp = &self.mountinfo[&uuid];
                if &mp != old_mp {
                    if send_events.send(MountChange::Modified(uuid, mp.clone())).is_err() {
                        return Ok(false);
                    }
                }
                new_map.insert(uuid, mp);
                seen_old.insert(uuid);
            } else {
                // Entirely new mount
                let uuid = Uuid::new_v4();
                if send_events.send(MountChange::Added(uuid, mp.clone())).is_err() {
                    return Ok(false);
                }
                new_map.insert(uuid, mp);
            }
        }

        // 4. Anything in the old state we *didn't* see above has been removed:
        for (&uuid, _) in &self.mountinfo {
            if !seen_old.contains(&uuid) {
                if send_events.send(MountChange::Removed(uuid)).is_err() {
                    return Ok(false);
                }
            }
        }

        // 5. Replace state
        self.mountinfo = new_map;

        Ok(true)
    }

    /// Sends all the stored mountpoints as newly `MountChange::Added`.
    ///
    /// Returns `false` if sending an event failed (sender is closed). `true` otherwise
    pub fn send_mountinfo(&self, send_events: &mut Sender<MountChange>) -> bool {
        for (uuid, mount) in &self.mountinfo {
            let change: MountChange = MountChange::Added(*uuid, mount.clone());
            if send_events.send(change).is_err() {
                return false;
            }
        }

        true
    }
}

pub fn monitor_mountinfo() -> Result<
    (
        Receiver<MountChange>,
        impl Future<Output = Result<(), Error>>,
    ),
    Error,
> {
    let mut monitor = SendMonitor::new();
    monitor.enable_kernel(true)?;
    monitor.enable_userspace(true, None)?;
    let (mut mount_stream, mount_fut) = monitor.stream()?;

    let (mut send, recv) = tokio::sync::broadcast::channel(1024);

    let mut state = State::new()?;

    let fut = async move {
        let mount_fut = tokio::spawn(mount_fut);

        let mut should_run = true;
        if !state.send_mountinfo(&mut send) {
            should_run = false;
        }

        'main: while should_run {
            tokio::select! {
                _ = send.closed() => break 'main,

                result = mount_stream.recv() => {
                    let Ok(event) = result else {
                        break 'main;
                    };
                    let mount_file = event.path;

                    if mount_file == PathBuf::from_str("/proc/self/mountinfo").unwrap() {
                        if !state.update_mountinfo(&mut send)? {
                            break 'main;
                        }
                    } else {
                        eprintln!("[Mount Monitor] Unexpected mount file received from libmount: {}", mount_file.display());
                    }
                }
            }
        }

        let _ = mount_fut.await;
        Ok(())
    };

    Ok((recv, fut))
}
