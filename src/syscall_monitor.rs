use std::{borrow::Cow, path::PathBuf, time::Duration};

use aya::{
    Ebpf, EbpfError,
    maps::{MapError, RingBuf},
    programs::{ProgramError, TracePoint},
};
use thiserror::Error;
use tokio::{
    io::unix::AsyncFd,
    sync::broadcast::{Receiver, Sender, error::SendError},
    time::sleep,
};

const TASK_COMM_LENGTH: usize = 16;

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum EventType {
    Fork = 0,
    Exec = 1,
    Exit = 2,
    Clone = 3,
    Unshare = 4,
    Setns = 5,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EbpfEvent {
    pub kind: EventType,
    pub pid: u32,
    pub tid: u32,
    pub uid: u32,
    pub gid: u32,
    pub parent_pid: u32,
    pub command: [u8; TASK_COMM_LENGTH],
}

impl EbpfEvent {
    pub fn command_as_string(&self) -> Cow<'_, str> {
        let len = self
            .command
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.command.len());
        String::from_utf8_lossy(&self.command[..len])
    }
}

impl std::fmt::Debug for EbpfEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EbpfEvent")
            .field("kind", &self.kind)
            .field("pid", &self.pid)
            .field("tid", &self.tid)
            .field("uid", &self.uid)
            .field("gid", &self.gid)
            .field("parent_pid", &self.parent_pid)
            .field("command", &self.command_as_string())
            .finish()
    }
}

fn get_object_path() -> std::io::Result<PathBuf> {
    let object_dir;

    match std::env::var("EBPF_OBJECT_DIR") {
        Ok(other) => {
            if other == "EXE_DIR" {
                eprintln!(
                    "Trying to load ebpf programs from current executable directory + /ebpf/"
                );
                object_dir = std::env::current_exe()?.parent().unwrap().join("ebpf");
            } else if other == "CUR_DIR" {
                eprintln!("Trying to load ebpf programs from working directory");
                object_dir = std::env::current_dir()?.join("ebpf");
            } else {
                object_dir = other.parse().unwrap();
            }
        }
        Err(_err) => {
            eprintln!(
                "EBPF_OBJECT_DIR is not set, trying to load ebpf programs from current executable directory"
            );
            object_dir = std::env::current_exe()?.parent().unwrap().join("ebpf");
        }
    }

    let filepath = object_dir.join("fork_monitor.bpf.o");

    Ok(filepath)
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error - {0}")]
    Io(#[from] std::io::Error),
    #[error("eBPF error - {0}")]
    Ebpf(#[from] EbpfError),
    #[error("program error - {0}")]
    Program(#[from] ProgramError),
    #[error("map error - {0}")]
    Map(#[from] MapError),
    #[error("send error - {0}")]
    Send(#[from] SendError<EbpfEvent>),
}

pub fn monitor_syscalls() -> Result<
    (
        Receiver<EbpfEvent>,
        impl Send + Future<Output = Result<(), Error>>,
    ),
    Error,
> {
    let mut bpf = Ebpf::load_file(get_object_path()?)?;

    // Attach fork tracepoint
    let attachments = [
        ("trace_sched_process_fork", "sched", "sched_process_fork"),
        ("trace_exec", "syscalls", "sys_enter_execve"),
        ("trace_exit", "sched", "sched_process_exit"),
        ("trace_clone", "syscalls", "sys_enter_clone"),
        ("trace_unshare", "syscalls", "sys_enter_unshare"),
        ("trace_setns", "syscalls", "sys_enter_setns"),
    ];
    for (program_name, category, attachment) in attachments {
        let program: &mut TracePoint = bpf.program_mut(program_name).unwrap().try_into()?;
        program.load()?;
        program.attach(category, attachment)?;
    }

    let (send, recv) = tokio::sync::broadcast::channel(1024);

    let fut = poll_messages(bpf, send);
    Ok((recv, fut))
}

async fn poll_messages(mut bpf: Ebpf, send: Sender<EbpfEvent>) -> Result<(), Error> {
    let ringbuf = RingBuf::try_from(bpf.map_mut("events").unwrap())?;
    let mut async_fd = AsyncFd::new(ringbuf)?;

    'main: loop {
        tokio::select! {
            _ = send.closed() => break 'main,

            guard = async_fd.readable_mut() => {
                let mut guard = guard?;
                while let Some(item) = guard.get_inner_mut().next() {
                    let event: EbpfEvent = unsafe { std::ptr::read(item.as_ptr() as *const _) };
                    match send.send(event) {
                        Ok(_) => {}
                        Err(_) => break 'main,
                    };
                }

                sleep(Duration::from_millis(1)).await;
            }
        }
    }

    Ok(())
}
