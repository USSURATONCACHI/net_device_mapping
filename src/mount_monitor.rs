use std::path::PathBuf;

use thiserror::Error;
use tokio::sync::broadcast::{Receiver, error::SendError};

use crate::util::SendMonitor;

#[derive(Debug, Clone)]
pub enum MountChange {
    Added(PathBuf),
    Removed(PathBuf),
    Modified(PathBuf),
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

    let (send, recv) = tokio::sync::broadcast::channel(1024);

    let fut = async move {
        let mount_fut = tokio::spawn(mount_fut);

        'main: loop {
            tokio::select! {
                _ = send.closed() => break 'main,

                result = mount_stream.recv() => {
                    let Ok(event) = result else {
                        break 'main;
                    };

                    if send.send(MountChange::Modified(event.path)).is_err() {
                        break 'main;
                    }

                    // TODO: Rescan the file in the path and compare with previous scan
                    // TODO: Custom scanners for each filepath. Default is /proc/self/mountinfo
                }
            }
        }

        let _ = mount_fut.await;
        Ok(())
    };

    Ok((recv, fut))
}
