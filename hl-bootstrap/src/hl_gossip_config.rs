use std::{net::Ipv4Addr, str::FromStr};

use eyre::{Context, ContextCompat, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;

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

#[derive(Clone, Debug)]
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
) -> eyre::Result<Vec<HyperliquidSeedPeer>> {
    if !matches!(chain, HyperliquidChain::Mainnet) {
        warn!(?chain, "no seed nodes source for chain");
        return Ok(Default::default());
    }

    // Unfortunately there is no other source as of 2025-07-23 for non-validating seed nodes,
    // so we have to extract these from README.md! Holy fucking shit honestly, but have to make do
    // with what we have.
    let url = "https://github.com/hyperliquid-dex/node/raw/refs/heads/main/README.md";

    // Fetch the README content
    let response = reqwest::get(url).await?;
    let content = response.text().await?;

    let mut in_block = false;
    let mut found_header = false;
    let mut csv_lines = Vec::new();

    for line in content.lines() {
        // Toggle block state when we encounter ```
        if line.trim() == "```" {
            if in_block && found_header {
                // We've reached the end of our target block
                break;
            }
            in_block = !in_block;
            continue;
        }

        // If we're in a code block and haven't found the header yet
        if in_block && !found_header && line.starts_with("operator_name,root_ips") {
            found_header = true;
            continue; // Skip the header line
        }

        // If we're in the block and have found the header, collect CSV lines
        if in_block && found_header {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                let mut split = trimmed.splitn(2, ',');
                let operator_name = split
                    .next()
                    .wrap_err("failed to get operator name from line")?;
                let raw_ip = split
                    .next()
                    .wrap_err("failed to get operator ip address from line")?;

                let ip: Ipv4Addr = raw_ip
                    .parse()
                    .wrap_err("failed to parse operator ipv4 address")?;

                csv_lines.push(HyperliquidSeedPeer {
                    operator_name: operator_name.to_string(),
                    ip,
                });
            }
        }
    }

    Ok(csv_lines)
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
        let seed_peers = fetch_hyperliquid_seed_peers(HyperliquidChain::Mainnet).await?;

        assert!(!seed_peers.is_empty(), "Should have at least one entry");

        println!("Found {} CSV entries", seed_peers.len());
        for (i, line) in seed_peers.iter().take(3).enumerate() {
            println!("Entry {}: {:?}", i + 1, line);
        }

        Ok(())
    }
}
