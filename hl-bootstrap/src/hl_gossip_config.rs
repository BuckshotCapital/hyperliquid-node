use std::{collections::HashSet, net::Ipv4Addr, str::FromStr};

use eyre::{Context, ContextCompat, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{debug, warn};

structstruck::strike! {
    #[structstruck::each[derive(Clone, Debug, Deserialize, Serialize)]]
    pub struct OverrideGossipConfig {
        #[serde(default)]
        pub root_node_ips: Vec<pub struct NodeIp {
            #[serde(rename = "Ip")]
            pub ip: Ipv4Addr,
        }>,
        #[serde(default)]
        pub try_new_peers: bool,
        pub chain: pub enum HyperliquidChain {
            #![derive(Copy)]

            #[serde(rename = "Mainnet")]
            Mainnet,
            #[serde(rename = "Testnet")]
            Testnet,
        },
        #[serde(skip_serializing_if = "Option::is_none")]
        pub n_gossip_peers: Option<u16>,
        #[serde(flatten, default)]
        pub unknown: Value,
    }
}

impl OverrideGossipConfig {
    pub fn new(chain: HyperliquidChain) -> Self {
        Self {
            root_node_ips: Default::default(),
            try_new_peers: true,
            chain,
            n_gossip_peers: None,
            unknown: Default::default(),
        }
    }
}

impl FromStr for HyperliquidChain {
    type Err = eyre::ErrReport;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "mainnet" => Self::Mainnet,
            "testnet" => Self::Testnet,
            chain => bail!("unsupported chain '{chain}'"),
        })
    }
}

#[allow(clippy::to_string_trait_impl)]
impl ToString for HyperliquidChain {
    fn to_string(&self) -> String {
        match self {
            Self::Mainnet => "Mainnet",
            Self::Testnet => "Testnet",
        }
        .to_string()
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HyperliquidSeedPeer {
    #[allow(dead_code)] // Keeping due to its value in logs
    pub operator_name: String,
    pub ip: Ipv4Addr,
}

impl From<HyperliquidSeedPeer> for NodeIp {
    fn from(value: HyperliquidSeedPeer) -> Self {
        Self { ip: value.ip }
    }
}

pub async fn fetch_hyperliquid_seed_peers(
    chain: HyperliquidChain,
    ignored_peers: &HashSet<Ipv4Addr>,
) -> eyre::Result<Vec<HyperliquidSeedPeer>> {
    match chain {
        HyperliquidChain::Mainnet => {
            let mut all_peers = HashSet::new();

            match fetch_mainnet_seed_peers_api(ignored_peers).await {
                Ok(peers) => all_peers.extend(peers),
                Err(err) => warn!(
                    ?err,
                    "failed to get usable mainnet peers from Hyperliquid API"
                ),
            }

            match fetch_mainnet_seed_peers_markdown_table(ignored_peers).await {
                Ok(peers) => all_peers.extend(peers),
                Err(err) => warn!(?err, "failed to get usable peers from markdown table"),
            };

            if all_peers.is_empty() {
                bail!("No usable seed peers found");
            }

            Ok(Vec::from_iter(all_peers))
        }
        HyperliquidChain::Testnet => fetch_testnet_seed_peers(ignored_peers).await,
    }
}

async fn fetch_mainnet_seed_peers_api(
    ignored_peers: &HashSet<Ipv4Addr>,
) -> eyre::Result<Vec<HyperliquidSeedPeer>> {
    let peer_ips: Vec<Ipv4Addr> = reqwest::Client::new()
        .post("https://api.hyperliquid.xyz/info")
        .json(&json!({"type": "gossipRootIps"}))
        .send()
        .await
        .wrap_err("failed to get mainnet seed nodes")?
        .error_for_status()
        .wrap_err("failed to get mainnet seed nodes")?
        .json()
        .await
        .wrap_err("failed to parse mainnet seed nodes")?;

    if peer_ips.is_empty() {
        bail!("No seed peers were given from Hyperliquid API");
    }

    let mut seeds = Vec::new();
    for ip in peer_ips {
        if ignored_peers.contains(&ip) {
            debug!(?ip, "skipping ignored seed node");
            continue;
        }

        seeds.push(HyperliquidSeedPeer {
            operator_name: "Hyperliquid API-provided IP".to_string(),
            ip,
        });
    }

    Ok(seeds)
}

async fn fetch_mainnet_seed_peers_markdown_table(
    ignored_peers: &HashSet<Ipv4Addr>,
) -> eyre::Result<Vec<HyperliquidSeedPeer>> {
    // There is an API request to fetch mainnet non-validating seed node IPs since 2025-09-02, but it'll only give us
    // JP IP addresses, which are usually unsuitable for syncing the node from EU.
    // Keep Markdown table parsing code around for now.
    let url = "https://github.com/hyperliquid-dex/node/raw/refs/heads/main/README.md";

    // Fetch the README content
    let response = reqwest::get(url).await?;
    let content = response.text().await?;

    let mut peers = Vec::new();

    // Find the table section that contains the seed peers
    // Look for the "Mainnet Non-Validator Seed Peers" section
    let seed_peers_section = content
        .split("## Mainnet Non-Validator Seed Peers")
        .nth(1)
        .wrap_err("could not find 'Mainnet Non-Validator Seed Peers' section from hyperliquid-dex/node README.md")?;

    // Split by next section (starts with ##) to isolate just the peers table
    let peers_content = seed_peers_section
        .split("##")
        .next()
        .unwrap_or(seed_peers_section);

    // Find the table by looking for lines that start and end with |
    let lines: Vec<&str> = peers_content.lines().collect();
    let mut in_table = false;
    let mut header_found = false;

    for line in lines {
        let trimmed = line.trim();

        // Skip empty lines
        if trimmed.is_empty() {
            continue;
        }

        // Check if this line looks like a markdown table row
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            // Skip separator lines (contains only |, -, and spaces)
            if trimmed
                .chars()
                .all(|c| c == '|' || c == '-' || c.is_whitespace())
            {
                in_table = true;
                continue;
            }

            // Parse the table cells
            let cells: Vec<&str> = trimmed
                .split('|')
                .map(|cell| cell.trim())
                .filter(|cell| !cell.is_empty())
                .collect();

            // Skip header row
            if !header_found
                && cells.len() >= 2
                && (cells[0].to_lowercase().contains("operator")
                    || cells[1].to_lowercase().contains("root")
                    || cells[1].to_lowercase().contains("ip"))
            {
                header_found = true;
                in_table = true;
                continue;
            }

            // Parse data rows
            if in_table && header_found && cells.len() >= 2 {
                let operator_name = cells[0].to_string();
                let ip_str = cells[1];

                // Parse the IP address
                match ip_str.parse::<Ipv4Addr>() {
                    Ok(ip) => {
                        if ignored_peers.contains(&ip) {
                            debug!(operator_name, ?ip, "skipping ignored seed node");
                            continue;
                        }

                        peers.push(HyperliquidSeedPeer { operator_name, ip });
                    }
                    Err(err) => {
                        debug!(?err, ip_str, "failed to parse ip");
                        continue;
                    }
                }
            }
        } else if in_table {
            // If we were in a table but this line doesn't look like a table row,
            // we've probably reached the end of the table
            break;
        }
    }

    if peers.is_empty() {
        bail!("No valid seed peers found in markdown table");
    }

    Ok(peers)
}

async fn fetch_testnet_seed_peers(
    ignored_peers: &HashSet<Ipv4Addr>,
) -> eyre::Result<Vec<HyperliquidSeedPeer>> {
    // Imperator.co is generous
    let url = "https://hyperliquid-testnet.imperator.co/peers.json";

    let config: OverrideGossipConfig = reqwest::get(url)
        .await
        .wrap_err("failed to get testnet seed nodes")?
        .error_for_status()?
        .json()
        .await
        .wrap_err("failed to parse testnet override_gossip_config")?;

    let operator_name = "Imperator.co";

    let mut seeds = Vec::new();
    for node in config.root_node_ips {
        if ignored_peers.contains(&node.ip) {
            debug!(operator_name, ip = ?node.ip, "skipping ignored seed node");
            continue;
        }

        seeds.push(HyperliquidSeedPeer {
            operator_name: operator_name.to_string(),
            ip: node.ip,
        });
    }

    Ok(seeds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_override_gossip_config() -> eyre::Result<()> {
        let config_snippet = r#"
            {
                "root_node_ips": [{"Ip": "1.2.3.4"}],
                "try_new_peers": false,
                "chain": "Mainnet",
                "reserved_peer_ips": ["5.6.7.8"]
            }
        "#;

        let config: OverrideGossipConfig = serde_json::from_str(config_snippet)?;
        dbg!(&config);
        let serialized = serde_json::to_string_pretty(&config)?;
        println!("{serialized}");

        Ok(())
    }

    // Requires network access
    #[tokio::test]
    async fn test_fetch_seed_peers() -> eyre::Result<()> {
        let ignored_peers = Default::default();
        let seed_peers =
            fetch_hyperliquid_seed_peers(HyperliquidChain::Mainnet, &ignored_peers).await?;

        assert!(!seed_peers.is_empty(), "Should have at least one entry");

        println!("Found {} CSV entries", seed_peers.len());
        for (i, line) in seed_peers.iter().take(3).enumerate() {
            println!("Entry {}: {:?}", i + 1, line);
        }

        Ok(())
    }
}
