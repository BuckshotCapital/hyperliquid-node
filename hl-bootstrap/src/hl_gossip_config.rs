use std::{collections::HashSet, net::Ipv4Addr, str::FromStr};

use eyre::{Context, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::debug;

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

#[derive(Clone, Debug)]
pub struct HyperliquidSeedPeer {
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
        HyperliquidChain::Mainnet => fetch_mainnet_seed_peers(ignored_peers).await,
        HyperliquidChain::Testnet => fetch_testnet_seed_peers(ignored_peers).await,
    }
}

async fn fetch_mainnet_seed_peers(
    ignored_peers: &HashSet<Ipv4Addr>,
) -> eyre::Result<Vec<HyperliquidSeedPeer>> {
    let peer_ips: Vec<Ipv4Addr> = reqwest::Client::new()
        .post("https://api.hyperliquid.xyz/info")
        .body(r#"{"type":"gossipRootIps"}"#)
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

        seeds.push(HyperliquidSeedPeer { ip });
    }

    Ok(seeds)
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

    let mut seeds = Vec::new();
    for node in config.root_node_ips {
        if ignored_peers.contains(&node.ip) {
            debug!(ip = ?node.ip, "skipping ignored seed node");
            continue;
        }

        seeds.push(HyperliquidSeedPeer { ip: node.ip });
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
}
