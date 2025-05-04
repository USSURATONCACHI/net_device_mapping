use net_device_mapping::util::StoppableStream;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let (events, fut) = net_device_mapping::mount_monitor::monitor_mountinfo()?;
    let (mut events, mut stop) = StoppableStream::new(events);

    tokio::spawn(fut);
    ctrlc::set_handler(move || stop.send(()).unwrap())?;

    println!("Monitoring mounting changes");
    while let Ok(event) = events.recv().await {
        println!("{event:?}");
    }

    Ok(())
}
