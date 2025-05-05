use std::time::Duration;

use net_device_mapping::util::{LineCountWriter, StoppableStream};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let (syscalls, syscalls_fut) = net_device_mapping::syscall_monitor::monitor_syscalls()?;
    let (nsid_events, nsid_fut) = net_device_mapping::nsid_monitor::monitor_netns_ids()?;
    let (mounts, mounts_fut) = net_device_mapping::mount_monitor::monitor_mountinfo()?;

    let (state_req_tx, state_rx, tracker_fut) =
        net_device_mapping::netns_tracker::monitor_network_namespaces(
            nsid_events,
            mounts,
            syscalls,
        )?;

    let handle =
        tokio::spawn(async move { tokio::join!(syscalls_fut, nsid_fut, mounts_fut, tracker_fut) });
    let (mut states, mut stop) = StoppableStream::new(state_rx);

    // Request a state every second.
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            if state_req_tx.send(()).is_err() {
                break;
            }
        }
    });

    ctrlc::set_handler(move || stop.send(()).unwrap())?;

    println!("Monitoring specific syscalls from all processes");

    let mut last_lines_count = None;
    while let Ok(mut namespaces) = states.recv().await {
        use std::io::Write;
        let mut writer = std::io::stdout().lock();

        if let Some(lines) = last_lines_count {
            if lines > 0 {
                clear_from_n_lines_above(&mut writer, lines)?;
            }
        }

        let mut writer = LineCountWriter::new(writer);
        writeln!(writer, "\n\n")?;
        writeln!(writer, "Namespaces: {}", namespaces.len())?;

        namespaces.sort_by_key(|n| n.inode);
        for mut netns in namespaces {
            netns.pids.sort();
            writeln!(
                writer,
                "Network namespace : INode = {}\t| Id = {}\t Path = {:?}\t| Pids: {}.",
                netns.inode,
                match netns.id {
                    Some(id) => id.to_string(),
                    None => "None".to_owned(),
                },
                netns.fs_path,
                netns.pids.len(),
            )?;
        }

        last_lines_count = Some(writer.into_inner().1 as u16);
    }

    // Make sure these future shut down gracefully
    let (r1, r2, r3, r4) = handle.await.unwrap();
    r1.unwrap();
    r2.unwrap();
    r3.unwrap();
    r4.unwrap();

    Ok(())
}

use crossterm::{
    cursor::MoveUp,
    execute,
    terminal::{Clear, ClearType},
};
use std::io::{self, Write};

/// Moves the cursor up `lines` rows, then clears from that line downwards.
pub fn clear_from_n_lines_above<W: Write>(w: &mut W, lines: u16) -> io::Result<()> {
    execute!(w, MoveUp(lines), Clear(ClearType::FromCursorDown),)
}
