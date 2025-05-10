use net_device_mapping::util::StoppableStream;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let (events, fut) = net_device_mapping::nsid_monitor::monitor_netns_ids()?;
    let (mut events, mut stop) = StoppableStream::new(events);

    tokio::spawn(fut);
    ctrlc::set_handler(move || stop.send(()).unwrap())?;

    println!("Monitoring namespaces id changes");
    while let Ok(event) = events.recv().await {
        println!("{event:?}");
    }

    Ok(())
}
