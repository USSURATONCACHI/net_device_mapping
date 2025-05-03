use cnproc::{PidEvent, PidMonitor};

pub async fn track_processes() -> std::io::Result<()> {
    let mut monitor = PidMonitor::new()?;

    while let Some(event) = monitor.recv() {
        match event {
            PidEvent::Exec { process_pid, process_tgid } => todo!(),
            PidEvent::Fork { child_pid, child_tgid, parent_pid, parent_tgid } => todo!(),
            PidEvent::Coredump { process_pid, process_tgid, parent_pid, parent_tgid } => todo!(),
            PidEvent::Exit { process_pid, process_tgid, parent_pid, parent_tgid, exit_code, exit_signal } => todo!(),
        }
    }

    Ok(())
}
