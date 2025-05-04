#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let (mut events, mut stop, fut) = 
        net_device_mapping::syscall_monitor::monitor_syscalls()?;
        
    tokio::spawn(fut);
    ctrlc::set_handler(move || stop.send(()).unwrap())?;

    while let Ok(event) = events.recv().await {
        println!("{event:?}");
    }

    Ok(())
}
