use std::{
    fs::{File, Permissions},
    io::{ErrorKind, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
};

use eyre::{Context, ContextCompat, bail};
use http::header::ETAG;
use tempfile::NamedTempFile;
use tokio::fs::{read_to_string, set_permissions};
use tracing::{debug, info, trace, warn};

use crate::hl_gossip_config::HyperliquidChain;

pub async fn download_hl_visor(
    base_path: impl AsRef<Path>,
    network: HyperliquidChain,
) -> eyre::Result<()> {
    let base_path = base_path.as_ref();

    debug!(?network, "checking for hl-visor updates");

    let binary_url = match network {
        HyperliquidChain::Mainnet => "https://binaries.hyperliquid.xyz/Mainnet/hl-visor",
        HyperliquidChain::Testnet => "https://binaries.hyperliquid-testnet.xyz/Testnet/hl-visor",
    };

    let hl_visor_path = base_path.join("hl-visor");
    let etag_file_path = base_path.join(".hl-visor.etag");

    let new_etag_value = fetch_etag(binary_url)
        .await
        .wrap_err("failed to obtain etag for hl-visor")?;

    let current_etag_value = match read_to_string(&etag_file_path).await {
        Ok(value) => Some(value.trim().to_string()),
        Err(err) if matches!(err.kind(), ErrorKind::NotFound) => None,
        Err(err) => {
            warn!(?err, ?etag_file_path, "failed to read last stored etag");
            None
        }
    };

    trace!(
        ?network,
        ?new_etag_value,
        ?current_etag_value,
        "comparing hl-visor etag values"
    );
    if matches!(&current_etag_value, Some(value) if *value == new_etag_value) {
        debug!(?network, etag = ?current_etag_value.unwrap(), "hl-visor appears up to date");
        return Ok(());
    }

    info!(?network, new_etag_value, "downloading new hl-visor binary");

    let mut new_binary = NamedTempFile::new_in(base_path)?;
    let mut new_sig_file = NamedTempFile::new_in(base_path)?;
    let mut new_etag_file = NamedTempFile::new_in(base_path)?;

    let binary_sig_url = format!("{binary_url}.asc");
    tokio::try_join!(
        download_file(binary_url, new_binary.as_file_mut()),
        download_file(&binary_sig_url, new_sig_file.as_file_mut())
    )?;

    // Verify hl-visor signature
    let gpg_result = Command::new("gpg")
        .arg("--verify")
        .arg(new_sig_file.path())
        .arg(new_binary.path())
        .output()?;
    if !gpg_result.status.success() {
        let stderr_str = str::from_utf8(&gpg_result.stderr);
        let stderr = match stderr_str {
            Ok(str) => str.to_string(),
            Err(_) => format!("{:?}", gpg_result.stderr),
        };

        bail!(
            "gpg verification for hl-visor failed with status {:?}:\n{}",
            gpg_result.status,
            stderr,
        );
    }

    // Persist hl-visor
    set_permissions(new_binary.path(), Permissions::from_mode(0o755)).await?;
    new_binary.flush()?;
    new_binary.persist(&hl_visor_path)?;

    // Store etag for future comparisons
    writeln!(&mut new_etag_file, "{new_etag_value}")?;
    new_etag_file.flush()?;
    new_etag_file.persist(etag_file_path)?;

    Ok(())
}

async fn fetch_etag(url: &str) -> eyre::Result<String> {
    trace!(?url, "fetching etag");

    let response = reqwest::Client::new()
        .head(url)
        .send()
        .await?
        .error_for_status()
        .wrap_err_with(|| format!("failed to send HEAD request to {url}"))?;

    let value = response
        .headers()
        .get(ETAG)
        .wrap_err_with(|| format!("no etag header available in HEAD {url} request"))?
        .to_str()
        .wrap_err("invalid etag header value")?;

    Ok(value.trim().to_string())
}

async fn download_file(url: &str, target: &mut File) -> eyre::Result<()> {
    let mut response = reqwest::get(url)
        .await?
        .error_for_status()
        .wrap_err_with(|| format!("failed to send GET request to {url}"))?;

    while let Some(chunk) = response.chunk().await? {
        target.write_all(&chunk)?;
    }
    target.flush()?;

    Ok(())
}
