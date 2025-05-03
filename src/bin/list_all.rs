use net_device_mapping::netns::NetworkNamespace;

#[tokio::main]
pub async fn main() {
    println!("Listing all network namespaces");

    let mut namespaces = NetworkNamespace::all().await.unwrap();

    namespaces.sort_by_key(|x| x.inode);

    for mut netns in namespaces {
        netns.pids.sort();
        println!(
            "Network namespace : INode = {}\t| Id = {}\t Path = {}\t| Pids ({}) = {:?}.",
            netns.inode,
            match netns.id {
                Some(id) => id.to_string(),
                None => "None".to_owned(),
            },
            match netns.fs_path {
                Some(path) => path.display().to_string(),
                None => "None".to_owned(),
            },
            netns.pids.len(),
            netns.pids
        );
    }
}
