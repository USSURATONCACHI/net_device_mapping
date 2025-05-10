use std::collections::HashSet;

use futures::{FutureExt, StreamExt, stream::FuturesUnordered};
use net_device_mapping::{net_device::query_netns_links, netns::NetworkNamespace};
use rtnetlink::packet_route::link::{InfoKind, LinkAttribute, LinkInfo};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    use std::io::Write;
    let mut links_file = std::fs::File::create("net_devices.log")?;

    let network_namespaces = NetworkNamespace::all().await?;

    let futures: FuturesUnordered<_> = network_namespaces
        .into_iter()
        .filter_map(|netns| netns.any_file().map(|file| (netns, file)))
        .map(|(netns, file)| query_netns_links(file).map(|x| (netns, x)))
        .collect();

    let mut results = futures.collect::<Vec<_>>().await;

    results.sort_by_key(|(netns, _)| netns.inode);

    for (netns, result) in results {
        println!(
            "\nNetwork namespace INode = {}, path = {:?}",
            netns.inode, netns.fs_path
        );

        writeln!(
            links_file,
            "\nNetwork namespace INode = {}, path = {:?}",
            netns.inode, netns.fs_path
        )?;

        match result {
            Ok(links) => {
                for link in links {
                    writeln!(links_file, "{link:?}")?;
                    let id = link.header.index;
                    let name = link_name(link.attributes.iter()).unwrap();
                    let kind = link_kind(link.attributes.iter());
                    let peers = link_peers(link.attributes.iter()).collect::<HashSet<_>>();

                    println!("\t- {name}\t: id = {id},\tkind = {kind:?},\tpeers = {peers:?}");
                }
            }
            Err(e) => println!("Error: {e}"),
        }
    }

    Ok(())
}

fn link_name<'a>(link: impl Iterator<Item = &'a LinkAttribute>) -> Option<&'a String> {
    link.filter_map(|x| {
        if let LinkAttribute::IfName(name) = x {
            Some(name)
        } else {
            None
        }
    })
    .next()
}

fn link_kind<'a>(link: impl Iterator<Item = &'a LinkAttribute>) -> Option<&'a InfoKind> {
    link.filter_map(|x| {
        if let LinkAttribute::LinkInfo(link_infos) = x {
            Some(link_infos)
        } else {
            None
        }
    })
    .filter_map(|x| {
        x.iter()
            .filter_map(|y| {
                if let LinkInfo::Kind(kind) = y {
                    Some(kind)
                } else {
                    None
                }
            })
            .next()
    })
    .next()
}

fn link_peers<'a>(
    attributes: impl Iterator<Item = &'a LinkAttribute>,
) -> impl Iterator<Item = u32> {
    attributes.filter_map(|x| {
        if let LinkAttribute::Link(id) = x {
            Some(*id)
        } else {
            None
        }
    })
}
