use std::{
    fs::File,
    io::Read,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use aya::{Ebpf, maps::RingBuf, programs::TracePoint};
use futures::future::abortable;
use tokio::io::unix::AsyncFd;

#[repr(C)]
struct ForkEvent {
    parent_pid: u32,
    child_pid: u32,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
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
                object_dir = other.parse()?;
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

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;

    println!("Loading program");
    let mut bpf = Ebpf::load_file(filepath)?;

    println!("Getting tracepoint");
    let program: &mut TracePoint = bpf
        .program_mut("trace_sched_process_fork")
        .unwrap()
        .try_into()?;
    program.load()?;
    program.attach("sched", "sched_process_fork")?;

    let ringbuf = RingBuf::try_from(bpf.map_mut("events").unwrap())?;
    let mut async_fd = AsyncFd::new(ringbuf)?;

    while running.load(Ordering::SeqCst) {
        let mut _guard = async_fd.readable().await?;

        if let Some(item) = async_fd.get_mut().next() {
            let event: ForkEvent = unsafe { std::ptr::read(item.as_ptr() as *const _) };
            println!("Fork: PID {} -> {}", event.parent_pid, event.child_pid);
        }
    }

    Ok(())
}
