use std::collections::HashMap;

use net_device_mapping::{netns::{INode, NetworkNamespace, Pid}, syscall_monitor::EbpfEvent};
use tokio::sync::broadcast::Receiver;

#[derive(Clone, Debug)]
pub struct ProcessState {
    command: String,
    netns: INode,
}

#[derive(Debug)]
pub struct ProcessTracker {
    network_namespaces: HashMap<INode, NetworkNamespace>,
    processes: HashMap<Pid, ProcessState>,
    events_recv: Receiver<EbpfEvent>,
}

impl ProcessTracker {
    pub fn new() -> Self {
        todo!()
    }

    pub fn init_from_procfs(&mut self) -> std::io::Result<()> {
        todo!()
    }
}

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
