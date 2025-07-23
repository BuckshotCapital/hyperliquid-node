use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use tokio::{
    net::TcpStream,
    sync::Semaphore,
    time::{Instant, timeout},
};
use tracing::{Level, debug, info, trace};

use crate::hl_gossip_config::HyperliquidSeedPeer;

// TODO: return failure reason for debugging
async fn measure_node_latency(
    ip: Ipv4Addr,
    port: u16,
    timeout_duration: Duration,
) -> Option<Duration> {
    let addr = SocketAddr::new(ip.into(), port);
    let start = Instant::now();

    match timeout(timeout_duration, TcpStream::connect(addr)).await {
        Ok(Ok(_)) => Some(start.elapsed()),
        _ => None, // Connection failed or timed out
    }
}

pub async fn speedtest_nodes(
    candidates: Vec<HyperliquidSeedPeer>,
    n: usize,
    timeout_duration: Duration,
) -> eyre::Result<Vec<HyperliquidSeedPeer>> {
    // NOTE: Gossip port is 4001 as of 2025-07-23, could change in the future
    let port = 4001;
    let concurrency = 64;

    info!(
        candidates = candidates.len(),
        concurrency, "testing latency to seed nodes"
    );

    // Use semaphore to limit concurrent connections
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut tasks = Vec::new();

    for (idx, node) in candidates.iter().enumerate() {
        let ip = node.ip;
        let sem = semaphore.clone();

        let task = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            let latency = measure_node_latency(ip, port, timeout_duration).await;
            (idx, latency)
        });

        tasks.push(task);
    }

    let mut successful_nodes = Vec::new();
    let mut failed = 0;

    for task in tasks {
        let (idx, latency) = task.await?;
        if let Some(latency) = latency {
            trace!(node = ?candidates[idx], ?latency, "latency test ok");
            successful_nodes.push((idx, latency));
        } else {
            trace!(node = ?candidates[idx], "latency test failed");
            failed += 1;
        }
    }

    info!(
        successful = successful_nodes.len(),
        failed = failed,
        "latency test complete"
    );

    // Sort by latency (lowest first)
    successful_nodes.sort_by(|a, b| a.1.cmp(&b.1));

    // NOTE: this could be more efficient, but I want to log all the nodes

    // Return the n lowest latency nodes
    let to_take = n.min(successful_nodes.len());
    let result: Vec<_> = successful_nodes
        .into_iter()
        .map(|(idx, latency)| (candidates[idx].clone(), latency)) // TODO: too lazy to remove this clone
        .collect();

    if tracing::enabled!(Level::DEBUG) {
        for (idx, (node, latency)) in result.iter().enumerate() {
            debug!(idx, ?node, ?latency, "seed node measurement");
        }
    }

    Ok(result
        .into_iter()
        .take(to_take)
        .enumerate()
        .map(|(idx, (node, latency))| {
            // Uh-oh, impure map fn
            info!(idx, ?node, ?latency, "picked seed node");

            node
        })
        .collect())
}
