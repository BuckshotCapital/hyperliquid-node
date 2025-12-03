use std::{io::Write, path::Path};

use eyre::{Context, ContextCompat};
use serde::Serialize;
use tempfile::NamedTempFile;

use crate::hl_gossip_config::HyperliquidChain;

#[derive(Debug, Serialize)]
pub struct VisorConfig {
    pub chain: HyperliquidChain,
}

pub fn write_hl_visor_config(
    path: impl AsRef<Path>,
    network: HyperliquidChain,
) -> eyre::Result<()> {
    let mut file =
        NamedTempFile::new_in(path.as_ref().parent().wrap_err("can't get parent path")?)?;

    serde_json::to_writer(file.as_file_mut(), &VisorConfig { chain: network })
        .wrap_err("failed to serialize hl-visor config")?;
    file.flush()?;

    file.persist(path)
        .wrap_err("failed to write hl-visor config")?;

    Ok(())
}
