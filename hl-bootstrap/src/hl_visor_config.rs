use std::{fs::File, path::PathBuf};

use eyre::{Context, ContextCompat};
use serde::Deserialize;
use tracing::debug;

use crate::hl_gossip_config::HyperliquidChain;

#[derive(Debug, Deserialize)]
pub struct VisorConfig {
    pub chain: HyperliquidChain,
}

pub fn read_hl_visor_config(config_file: Option<&PathBuf>) -> eyre::Result<VisorConfig> {
    let config_file = match config_file {
        Some(config_file) => config_file,
        None => {
            // NOTE: hl-visor expects visor.json next to itself as of 2025-07-23
            let path = which::which("hl-visor")?;
            debug!(?path, "found hl-visor in PATH");

            let hl_visor_dir = path
                .parent()
                .wrap_err("failed to determine hl-visor directory")?;

            &hl_visor_dir.join("visor.json")
        }
    };

    let config = File::open(config_file)
        .wrap_err_with(|| format!("failed to open hl-visor config at {config_file:?}"))?;

    debug!(?config_file, "found hl-visor config file");
    let config: VisorConfig = serde_json::from_reader(config)
        .wrap_err_with(|| format!("failed to parse hl-visor config at {config_file:?}"))?;

    Ok(config)
}
