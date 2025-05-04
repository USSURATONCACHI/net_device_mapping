use std::collections::HashSet;
use std::ffi::{CStr, CString, c_char, c_int};
use std::os::{fd::RawFd, unix::ffi::OsStrExt};
use std::time::Duration;
use std::{path::PathBuf, ptr::null};

use libc::c_uint;
use libmount_sys::libmnt_monitor;
use tokio::sync::broadcast::Receiver;
use tokio::time::sleep;

#[repr(u32)]
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventKind {
    Kernel = libmount_sys::MNT_MONITOR_TYPE_KERNEL,
    Userspace = libmount_sys::MNT_MONITOR_TYPE_USERSPACE,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Event {
    pub path: PathBuf,
    pub kind: EventKind,
}

/// Safe wrapper around libmnt_monitor. Is internally refcounted.
/// Cloning it will increment refcount.
///
/// Not `Send` nor `Sync` by original design due to reference counting.
/// It is safe to `Send` and `Sync` it when is it never cloned.
pub struct RcMonitor(*mut libmnt_monitor);

impl Drop for RcMonitor {
    fn drop(&mut self) {
        unsafe { libmount_sys::mnt_unref_monitor(self.0) };
    }
}

impl Clone for RcMonitor {
    fn clone(&self) -> Self {
        unsafe { libmount_sys::mnt_ref_monitor(self.0) };
        Self(self.0)
    }
}

impl RcMonitor {
    pub fn new() -> Self {
        Self(unsafe { libmount_sys::mnt_new_monitor() })
    }

    pub unsafe fn from_inner(mon: *mut libmnt_monitor) -> Self {
        Self(mon)
    }

    pub unsafe fn into_inner(self) -> *mut libmnt_monitor {
        self.0
    }

    /// It is only safe to do, when this instance of `RcMonitor` was never cloned.
    pub unsafe fn into_send(self) -> SendMonitor {
        SendMonitor(self)
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-enable-userspace>
    ///
    /// Enables or disables userspace monitoring. If the userspace monitor does not exist and enable=1 then allocates new resources necessary for the monitor.
    ///
    /// If the top-level monitor has been already created (by mnt_monitor_get_fd() or mnt_monitor_wait()) then it's updated according to enable .
    ///
    /// The filename is used only the first time when you enable the monitor. It's impossible to have more than one userspace monitor. The recommended is to use NULL as filename.
    ///
    /// The userspace monitor is unsupported for systems with classic regular /etc/mtab file.
    pub fn enable_userspace(
        &mut self,
        enable: bool,
        filename: Option<PathBuf>,
    ) -> std::io::Result<()> {
        let enable = if enable { 1 } else { 0 };
        let cstring_filename =
            filename.map(|filename| CString::new(filename.as_os_str().as_bytes()).unwrap());

        let code = unsafe {
            libmount_sys::mnt_monitor_enable_userspace(
                self.0,
                enable,
                cstring_filename
                    .map(|x| x.as_c_str().as_ptr())
                    .unwrap_or_else(|| null()),
            )
        };

        match code {
            0 => Ok(()),
            neg_errno if neg_errno < 0 => Err(std::io::Error::from_raw_os_error(-neg_errno)),
            _ => panic!("Undefined behaviour return code received from libmount"),
        }
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-enable-kernel>
    ///
    /// Enables or disables kernel VFS monitoring. If the monitor does not exist and enable=1 then allocates new resources necessary for the monitor.
    ///
    /// If the top-level monitor has been already created (by mnt_monitor_get_fd() or mnt_monitor_wait()) then it's updated according to enable .
    ///
    /// Return: 0 on success and <0 on error
    pub fn enable_kernel(&mut self, enable: bool) -> std::io::Result<()> {
        let enable = if enable { 1 } else { 0 };

        let code = unsafe { libmount_sys::mnt_monitor_enable_kernel(self.0, enable) };

        match code {
            0 => Ok(()),
            neg_errno if neg_errno < 0 => Err(std::io::Error::from_raw_os_error(-neg_errno)),
            _ => panic!("Undefined behaviour return code received from libmount"),
        }
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-get-fd>
    ///
    /// The file descriptor is associated with all monitored files and it's usable for example for epoll. You have to call mnt_monitor_event_cleanup() or mnt_monitor_next_change() after each event.
    pub fn get_fd(&mut self) -> std::io::Result<RawFd> {
        let fd = unsafe { libmount_sys::mnt_monitor_get_fd(self.0) };
        if fd >= 0 {
            Ok(fd)
        } else {
            Err(std::io::Error::from_raw_os_error(-fd))
        }
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-close-fd>
    ///
    /// Close monitor file descriptor. This is usually unnecessary, because mnt_unref_monitor() cleanups all.
    ///
    /// The function is necessary only if you want to reset monitor setting. The next mnt_monitor_get_fd() or mnt_monitor_wait() will use newly initialized monitor. This restart is unnecessary for mnt_monitor_enable_*() functions.
    pub fn close_fd(&mut self) -> std::io::Result<()> {
        let code = unsafe { libmount_sys::mnt_monitor_close_fd(self.0) };
        match code {
            0 => Ok(()),
            neg_errno if neg_errno < 0 => Err(std::io::Error::from_raw_os_error(-neg_errno)),
            _ => panic!("Undefined behaviour return code received from libmount"),
        }
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-next-change>
    ///
    /// The function does not wait and it's designed to provide details about changes. It's always recommended to use this function to avoid false positives.
    pub fn next_change(&mut self) -> std::io::Result<Option<Event>> {
        let mut path_ptr: *const c_char = std::ptr::null();
        let mut etype: c_int = 0;
        let result_code =
            unsafe { libmount_sys::mnt_monitor_next_change(self.0, &mut path_ptr, &mut etype) };
        if result_code == 0 {
            let path =
                unsafe { PathBuf::from(CStr::from_ptr(path_ptr).to_string_lossy().into_owned()) };
            let kind = match etype {
                x if x == libmount_sys::MNT_MONITOR_TYPE_KERNEL as c_int => EventKind::Kernel,
                x if x == libmount_sys::MNT_MONITOR_TYPE_USERSPACE as c_int => EventKind::Userspace,
                other => panic!("Unknown event kind returned from libmount: {other}"),
            };

            Ok(Some(Event { path, kind }))
        } else if result_code == 1 {
            // no more changes
            Ok(None)
        } else if result_code < 0 {
            let errno = -result_code;
            Err(std::io::Error::from_raw_os_error(errno))
        } else {
            panic!("Undefined behaviour return code received from libmount");
        }
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-event-cleanup>
    ///
    /// This function cleanups (drain) internal buffers. It's necessary to call this function after event if you do not call mnt_monitor_next_change().
    pub fn event_cleanup(&mut self) -> std::io::Result<()> {
        let code = unsafe { libmount_sys::mnt_monitor_event_cleanup(self.0) };
        match code {
            0 => Ok(()),
            neg_errno if neg_errno < 0 => Err(std::io::Error::from_raw_os_error(-neg_errno)),
            _ => panic!("Undefined behaviour return code received from libmount"),
        }
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-wait>
    ///
    /// Waits for the next change, after the event it's recommended to use mnt_monitor_next_change() to get more details about the change and to avoid false positive events.
    ///
    /// Returns `true` on success (something changed) or `false` on timeout.
    pub fn wait(&mut self, timeout: Timeout) -> std::io::Result<bool> {
        let timeout: c_int = match timeout {
            Timeout::Forever => -1,
            Timeout::Millis(n) => {
                if n == c_uint::MAX {
                    (n - 1) as c_int
                } else {
                    n as c_int
                }
            }
        };

        let code = unsafe { libmount_sys::mnt_monitor_wait(self.0, timeout) };
        match code {
            1 => Ok(true),
            0 => Ok(false),
            neg_errno if neg_errno < 0 => Err(std::io::Error::from_raw_os_error(-neg_errno)),
            _ => panic!("Undefined behaviour return code received from libmount"),
        }
    }
}

impl RcMonitor {
    /// [Non-Official]: Custom addition
    ///
    /// Creates a stream (polled by returned future) that will monitor all changes in real time.
    ///
    /// ```rust,no_run
    /// # use net_device_mapping::util::RcMonitor;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let mut monitor = RcMonitor::new();
    ///     monitor.enable_kernel(true).unwrap();
    ///     let (mut events, fut) = monitor.stream().unwrap();
    ///     
    ///     // Use [`SendMonitor`] instead of [`RcMonitor`] to be able to use `tokio::spawn()`
    ///     tokio::task::spawn_local(fut);
    ///     
    ///     while let Ok(event) = events.recv().await {
    ///         println!("{event:?}");
    ///     }
    /// }
    /// ```
    ///
    pub fn stream(
        mut self,
    ) -> std::io::Result<(Receiver<Event>, impl Future<Output = std::io::Result<()>>)> {
        let fd: RawFd = self.get_fd()?;

        let (send, recv) = tokio::sync::broadcast::channel(1024);

        let fut = async move {
            use tokio::io::unix::AsyncFd;
            let mut afd = AsyncFd::new(fd)?;

            'main: loop {
                tokio::select! {
                    _ = send.closed() => {
                        break 'main;
                    }
                    _ = afd.readable_mut() => {
                        let mut changed_files = HashSet::<Event>::new();

                        while let Ok(Some(event)) = self.next_change() {
                            if changed_files.insert(event.clone()) {
                                match send.send(event) {
                                    Ok(_) => {},
                                    Err(_) => break 'main, // No more receivers
                                }
                            }
                        }

                        sleep(Duration::from_millis(1)).await;
                    }
                }
            }

            // Clean up
            self.event_cleanup()?;
            Ok(())
        };

        Ok((recv, fut))
    }
}

pub enum Timeout {
    Forever,
    Millis(c_uint),
}

pub struct SendMonitor(RcMonitor);

unsafe impl Send for SendMonitor {}
unsafe impl Sync for SendMonitor {}

impl SendMonitor {
    pub fn new() -> Self {
        Self(RcMonitor::new())
    }

    pub unsafe fn from_inner(mon: RcMonitor) -> Self {
        Self(mon)
    }

    pub fn into_inner(self) -> RcMonitor {
        self.0
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-enable-userspace>
    ///
    /// Enables or disables userspace monitoring. If the userspace monitor does not exist and enable=1 then allocates new resources necessary for the monitor.
    ///
    /// If the top-level monitor has been already created (by mnt_monitor_get_fd() or mnt_monitor_wait()) then it's updated according to enable .
    ///
    /// The filename is used only the first time when you enable the monitor. It's impossible to have more than one userspace monitor. The recommended is to use NULL as filename.
    ///
    /// The userspace monitor is unsupported for systems with classic regular /etc/mtab file.
    pub fn enable_userspace(
        &mut self,
        enable: bool,
        filename: Option<PathBuf>,
    ) -> std::io::Result<()> {
        self.0.enable_userspace(enable, filename)
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-enable-kernel>
    ///
    /// Enables or disables kernel VFS monitoring. If the monitor does not exist and enable=1 then allocates new resources necessary for the monitor.
    ///
    /// If the top-level monitor has been already created (by mnt_monitor_get_fd() or mnt_monitor_wait()) then it's updated according to enable .
    ///
    /// Return: 0 on success and <0 on error
    pub fn enable_kernel(&mut self, enable: bool) -> std::io::Result<()> {
        self.0.enable_kernel(enable)
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-get-fd>
    ///
    /// The file descriptor is associated with all monitored files and it's usable for example for epoll. You have to call mnt_monitor_event_cleanup() or mnt_monitor_next_change() after each event.
    pub fn get_fd(&mut self) -> std::io::Result<RawFd> {
        self.0.get_fd()
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-close-fd>
    ///
    /// Close monitor file descriptor. This is usually unnecessary, because mnt_unref_monitor() cleanups all.
    ///
    /// The function is necessary only if you want to reset monitor setting. The next mnt_monitor_get_fd() or mnt_monitor_wait() will use newly initialized monitor. This restart is unnecessary for mnt_monitor_enable_*() functions.
    pub fn close_fd(&mut self) -> std::io::Result<()> {
        self.0.close_fd()
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-next-change>
    ///
    /// The function does not wait and it's designed to provide details about changes. It's always recommended to use this function to avoid false positives.
    pub fn next_change(&mut self) -> std::io::Result<Option<Event>> {
        self.0.next_change()
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-event-cleanup>
    ///
    /// This function cleanups (drain) internal buffers. It's necessary to call this function after event if you do not call mnt_monitor_next_change().
    pub fn event_cleanup(&mut self) -> std::io::Result<()> {
        self.0.event_cleanup()
    }

    /// <https://cdn.kernel.org/pub/linux/utils/util-linux/v2.37/libmount-docs/libmount-Monitor.html#mnt-monitor-wait>
    ///
    /// Waits for the next change, after the event it's recommended to use mnt_monitor_next_change() to get more details about the change and to avoid false positive events.
    ///
    /// Returns `true` on success (something changed) or `false` on timeout.
    pub fn wait(&mut self, timeout: Timeout) -> std::io::Result<bool> {
        self.0.wait(timeout)
    }
}

impl SendMonitor {
    /// [Non-Official]: Custom addition
    ///
    /// Creates a stream (polled by returned future) that will monitor all changes in real time.
    ///
    /// ```rust,no_run
    /// # use net_device_mapping::util::SendMonitor;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let mut monitor = SendMonitor::new();
    ///     monitor.enable_kernel(true).unwrap();
    ///     let (mut events, fut) = monitor.stream().unwrap();
    ///     tokio::spawn(fut);
    ///     
    ///     while let Ok(event) = events.recv().await {
    ///         println!("{event:?}");
    ///     }
    /// }
    /// ```
    ///
    pub fn stream(
        mut self,
    ) -> std::io::Result<(Receiver<Event>, impl Future<Output = std::io::Result<()>>)> {
        let fd: RawFd = self.get_fd()?;

        let (send, recv) = tokio::sync::broadcast::channel(1024);

        let fut = async move {
            use tokio::io::unix::AsyncFd;
            let mut afd = AsyncFd::new(fd)?;

            'main: loop {
                tokio::select! {
                    _ = send.closed() => {
                        break 'main;
                    }
                    _ = afd.readable_mut() => {
                        let mut changed_files = HashSet::<Event>::new();

                        while let Ok(Some(event)) = self.next_change() {
                            if changed_files.insert(event.clone()) {
                                match send.send(event) {
                                    Ok(_) => {},
                                    Err(_) => break 'main, // No more receivers
                                }
                            }
                        }

                        sleep(Duration::from_millis(1)).await;
                    }
                }
            }

            // Clean up
            self.event_cleanup()?;
            Ok(())
        };

        Ok((recv, fut))
    }
}
