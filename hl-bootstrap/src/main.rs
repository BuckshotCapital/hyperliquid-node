use std::{
    collections::HashSet,
    env::current_dir,
    ffi::OsString,
    fs::{self},
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    process::Command,
};

use clap::Parser;
use duration_string::DurationString;
use eyre::{Context, ContextCompat, bail};
use tempfile::NamedTempFile;
use tokio::runtime::{Builder, Runtime};
use tracing::{debug, error, info, level_filters::LevelFilter, trace, warn};
use tracing_subscriber::{
    EnvFilter,
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

mod hl_gossip_config;
mod hl_visor_config;
mod monitor;
mod prune;
mod speedtest;
mod sysctl;

use crate::{
    hl_gossip_config::{
        HyperliquidChain, HyperliquidSeedPeer, OverrideGossipConfig, fetch_hyperliquid_seed_peers,
    },
    hl_visor_config::read_hl_visor_config,
    prune::prune_worker_task,
    speedtest::speedtest_nodes,
    sysctl::read_sysctl,
};

#[derive(Clone, Debug, Parser)]
struct Cli {
    /// visor.json path, used to determine the network to use
    #[arg(long, env = "HL_BOOTSTRAP_VISOR_CONFIG_PATH")]
    visor_config_path: Option<PathBuf>,

    /// override_gossip_config.json path
    #[arg(
        long,
        env = "HL_BOOTSTRAP_OVERRIDE_GOSSIP_CONFIG_PATH",
        default_value = "./override_gossip_config.json"
    )]
    override_gossip_config_path: PathBuf,

    /// override_gossip_config.json max age when new peers will be checked & set up
    #[arg(
        long,
        env = "HL_BOOTSTRAP_OVERRIDE_GOSSIP_CONFIG_MAX_AGE",
        default_value = "15m"
    )]
    override_gossip_config_max_age: DurationString,

    /// How many seed peers to keep in the configuration
    #[arg(long, env = "HL_BOOTSTRAP_SEED_PEERS_AMOUNT", default_value_t = 5)]
    seed_peers_amount: usize,

    /// Maximum latency of seed peers to consider. Set to 80ms to prevent cross-continent connections by default (majority of the nodes are in Tokyo)
    #[arg(
        long,
        env = "HL_BOOTSTRAP_SEED_PEERS_MAX_LATENCY",
        default_value = "80ms"
    )]
    seed_peers_max_latency: DurationString,

    /// Ignore known bad seed peers by IP
    #[arg(long, env = "HL_BOOTSTRAP_SEED_PEERS_IGNORED", value_delimiter = ',')]
    seed_peers_ignored: Vec<Ipv4Addr>,

    /// Extra seed peers to consider
    #[arg(long, env = "HL_BOOTSTRAP_SEED_PEERS_EXTRA", value_delimiter = ',')]
    seed_peers_extra: Vec<Ipv4Addr>,

    /// Whether to ignore net.ipv6.conf.all.disable_ipv6 == 1. Due to hl-node bug, IPv6 being available to the node breaks it.
    #[arg(
        long,
        env = "HL_BOOTSTRAP_IGNORE_IPv6_ENABLED",
        default_value_t = false
    )]
    ignore_ipv6_enabled: bool,

    /// Whether to spawn data directory pruning task. This is used when hl-bootstrap has child process to execute
    #[arg(long, env = "HL_BOOTSTRAP_PRUNE_DATA_INTERVAL")]
    prune_data_interval: Option<DurationString>,

    /// Whether to prune data older than the specified duration
    #[arg(long, env = "HL_BOOTSTRAP_PRUNE_DATA_OLDER_THAN", default_value = "4h")]
    prune_data_older_than: DurationString,

    /// Whether to enable Prometheus metrics collection
    #[arg(long, env = "HL_BOOTSTRAP_METRICS_LISTEN_ADDRESS")]
    metrics_listen_address: Option<SocketAddr>,

    /// How often should the /info exchangeStatus request be done
    #[arg(
        long,
        env = "HL_BOOTSTRAP_METRICS_STATUS_POLL_INTERVAL",
        default_value = "100ms"
    )]
    metrics_status_poll_interval: DurationString,

    /// How much is node allowed to be behind the system time before reporting it unhealthy
    #[arg(
        long,
        env = "HL_BOOTSTRAP_METRICS_HEALTHY_DRIFT_THRESHOLD",
        default_value = "2500ms"
    )]
    metrics_healthy_drift_threshold: DurationString,

    /// Chain to set up configuration for
    #[arg(long, env = "HL_BOOTSTRAP_NETWORK")]
    network: Option<HyperliquidChain>,

    /// Free form args to execute after the setup
    args: Vec<OsString>,
}

fn main() -> eyre::Result<()> {
    let args = Cli::parse();

    // As hl-bootstrap is usually used in ENTRYPOINT, then bail out when first argument is
    // obviously not related to Hyperliquid (e.g. just running bash, for whatever purpose)
    if let Some(first_arg) = args.args.first()
        && first_arg != "run-non-validator"
        && first_arg != "run-validator"
    {
        let err = exec::Command::new(&args.args[0])
            .args(&args.args[1..])
            .exec();

        eprintln!("{err}");
        std::process::exit(1);
    }

    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(|| Box::new(std::io::stderr()))
                .with_target(true)
                .with_span_events(FmtSpan::CLOSE),
        )
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    trace!(?args, "args");

    let use_mt = args.prune_data_interval.is_some() || args.metrics_listen_address.is_some();

    let runtime = if use_mt {
        Builder::new_multi_thread()
    } else {
        Builder::new_current_thread()
    }
    .enable_all()
    .build()?;
    runtime.block_on(prepare_hl_node(&args))?;

    if args.args.is_empty() {
        info!("setup done");
        return Ok(());
    }

    run_node(runtime, &args)?;

    Ok(())
}

fn run_node(rt: Runtime, args: &Cli) -> eyre::Result<()> {
    info!(args = ?args.args, "setup done, executing hl-visor");

    if args.prune_data_interval.is_none() && args.metrics_listen_address.is_none() {
        // Just exec into the child
        let err = exec::Command::new("hl-visor").args(&args.args).exec();
        error!(?err, ?args.args, "failed to exec");
        std::process::exit(1);
    }

    // TODO: configurable in future
    let data_directory = current_dir().wrap_err("failed to get current working directory")?;

    let _prune_task = args.prune_data_interval.map(|prune_interval| {
        rt.spawn({
            let prune_data_older_than = args.prune_data_older_than;

            prune_worker_task(
                data_directory,
                prune_interval.into(),
                prune_data_older_than.into(),
            )
        })
    });

    let _poll_task = args.metrics_listen_address.is_some().then(|| {
        rt.spawn(crate::monitor::poll_node(
            args.metrics_status_poll_interval.into(),
        ))
    });

    let _metrics_server = args.metrics_listen_address.map(|address| {
        let metrics_healthy_drift_threshold = args.metrics_healthy_drift_threshold.into();
        rt.spawn(async move {
            info!(%address, "starting metrics server");
            if let Err(err) =
                crate::monitor::server::run_metrics_server(address, metrics_healthy_drift_threshold)
                    .await
            {
                error!(?err, "failed to start metrics server")
            }
        })
    });

    let mut child = Command::new("hl-visor")
        .args(&args.args)
        .spawn()
        .wrap_err("failed to spawn child")?;

    child.wait().wrap_err("failed to wait for child")?;

    Ok(())
}

async fn prepare_hl_node(args: &Cli) -> eyre::Result<()> {
    if !args.ignore_ipv6_enabled {
        let key_ipv6_all = "net.ipv6.conf.all.disable_ipv6";
        if let Ok(value) = read_sysctl(key_ipv6_all)
            && value == "0"
        {
            warn!(
                key = key_ipv6_all,
                value, "ipv6 appears to be enabled, node might not start up properly"
            );
        }
    }

    let network = match args.network {
        Some(network) => {
            debug!(?network, "network specified via args");
            network
        }
        None => {
            debug!("no network specified, reading from hl-visor configuration");
            let config = read_hl_visor_config(args.visor_config_path.as_ref())?;

            debug!(network = ?config.chain, "read hl-visor configuration");
            config.chain
        }
    };
    info!(?network, "preparing hl-node configuration");

    let ignored_seed_peers = HashSet::from_iter(args.seed_peers_ignored.clone());

    if let Ok(metadata) = fs::metadata(&args.override_gossip_config_path)
        && metadata.is_file()
    {
        let mtime = metadata.modified()?;
        let last_modified = mtime.elapsed().unwrap_or_default();

        debug!(
            ?last_modified,
            max_age = ?args.override_gossip_config_max_age,
            gossip_config_path = ?args.override_gossip_config_path,
            "gossip config last modified"
        );
        if last_modified <= args.override_gossip_config_max_age {
            debug!(
                ?mtime,
                gossip_config_path = ?args.override_gossip_config_path,
                "gossip config modified recently, not updating seed peers"
            );
            return Ok(());
        }
    }

    let config_path_directory = args
        .override_gossip_config_path
        .parent()
        .wrap_err("failed to determine override_gossip_config.json directory")?;

    // TODO: load existing configuration
    let mut config = OverrideGossipConfig::new(network);

    info!(?network, ?ignored_seed_peers, "fetching seed nodes");
    let mut seed_nodes = fetch_hyperliquid_seed_peers(network, &ignored_seed_peers).await?;
    info!(?network, count = seed_nodes.len(), "got seed nodes");

    if !args.seed_peers_extra.is_empty() {
        info!(
            ?network,
            count = args.seed_peers_extra.len(),
            "including extra seed peers from args"
        );
        for extra_seed in &args.seed_peers_extra {
            seed_nodes.push(HyperliquidSeedPeer {
                operator_name: "manual".to_string(),
                ip: *extra_seed,
            });
        }
    }

    if !seed_nodes.is_empty() {
        let tested_seed_nodes = speedtest_nodes(
            seed_nodes,
            args.seed_peers_amount,
            args.seed_peers_max_latency.into(),
        )
        .await
        .wrap_err("failed to measure latency of seed nodes")?;

        if tested_seed_nodes.is_empty() {
            bail!(
                "no seed nodes passed latency threshold, try increasing threshold (current: {})",
                args.seed_peers_max_latency
            );
        }

        for seed in tested_seed_nodes {
            config.root_node_ips.push(seed.into());
        }

        // Adjust n_gossip_peers
        // Allowed range is [1, 100]
        // See https://github.com/hyperliquid-dex/node/blob/main/README_misc.md#additional-configuration
        let n_gossip_peers = config.root_node_ips.len();
        if n_gossip_peers > 8 {
            config.n_gossip_peers = Some(n_gossip_peers.min(100) as u16);
        }
    }

    let mut new_config_file = NamedTempFile::new_in(config_path_directory)?;
    serde_json::to_writer(&mut new_config_file, &config)
        .wrap_err("failed to write new configuration")?;

    new_config_file
        .persist(&args.override_gossip_config_path)
        .wrap_err("failed to replace override_gossip_config.json")?;

    Ok(())
}
