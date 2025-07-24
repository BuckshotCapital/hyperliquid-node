use std::{
    sync::LazyLock,
    time::{Duration, SystemTime},
};

use prometheus::{
    Gauge, Histogram, IntGauge, exponential_buckets, histogram_opts, register_gauge,
    register_histogram, register_int_gauge,
};
use reqwest::{Client, ClientBuilder, Method, header::CONTENT_TYPE};
use serde::Deserialize;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{trace, warn};

pub mod server;

pub static GAUGE_HL_NODE_SYSTEM_TIME_MS: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge!(
        "hl_node_system_time",
        "Last reported system time in milliseconds since Unix epoch"
    )
    .unwrap()
});

pub static GAUGE_HL_NODE_TIME_MS: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge!(
        "hl_node_exchange_time",
        "Last reported HyperCore exchange time in milliseconds since Unix epoch"
    )
    .unwrap()
});

pub static GAUGE_HL_NODE_RESPONDING: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "hl_node_responding",
        "Whether HyperCore info endpoint is responding"
    )
    .unwrap()
});

pub static HISTOGRAM_HL_NODE_TIME_DRIFT_MS: LazyLock<Histogram> = LazyLock::new(|| {
    register_histogram!(histogram_opts!(
        "hl_node_time_drift",
        "HyperCore exchange time difference from system time in milliseconds",
        exponential_buckets(1.0, 1.25, 48).unwrap()
    ))
    .unwrap()
});

fn init_metrics() {
    LazyLock::force(&GAUGE_HL_NODE_SYSTEM_TIME_MS);
    LazyLock::force(&GAUGE_HL_NODE_TIME_MS);
    LazyLock::force(&GAUGE_HL_NODE_RESPONDING);
    LazyLock::force(&HISTOGRAM_HL_NODE_TIME_DRIFT_MS);
}

static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    ClientBuilder::new()
        .timeout(Duration::from_millis(100))
        .build()
        .unwrap()
});

async fn request_exchange_time() -> Result<u64, reqwest::Error> {
    #[derive(Deserialize)]
    struct ExchangeStatus {
        time: u64,
    }

    let status = CLIENT
        .request(Method::POST, "http://127.0.0.1:3001/info")
        .body(r#"{"type":"exchangeStatus"}"#)
        .header(CONTENT_TYPE, "application/json")
        .send()
        .await?
        .error_for_status()?
        .json::<ExchangeStatus>()
        .await?;

    Ok(status.time)
}

pub async fn poll_node(poll_interval: Duration) {
    init_metrics();

    let mut interval = interval(poll_interval);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut n = 1;
    loop {
        interval.tick().await;

        let system_now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();

        GAUGE_HL_NODE_SYSTEM_TIME_MS.set(as_ms_f64(&system_now));
        let exchange_now = match request_exchange_time().await {
            Ok(time) => Duration::from_millis(time),
            // Node is simply unavailable
            Err(err) if err.is_request() => {
                GAUGE_HL_NODE_RESPONDING.set(0);
                continue;
            }
            Err(err) => {
                if n % 50 == 0 {
                    warn!(%err, "unable to request exchange status from hl-node");
                    n = 0;
                }
                n += 1;
                GAUGE_HL_NODE_RESPONDING.set(0);
                continue;
            }
        };

        GAUGE_HL_NODE_RESPONDING.set(1);
        GAUGE_HL_NODE_TIME_MS.set(as_ms_f64(&exchange_now));
        let time_delta = system_now.saturating_sub(exchange_now);
        trace!(?time_delta, as_ms_f64 = as_ms_f64(&time_delta));
        HISTOGRAM_HL_NODE_TIME_DRIFT_MS.observe(as_ms_f64(&time_delta));
    }
}

#[inline]
const fn as_ms_f64(duration: &Duration) -> f64 {
    (duration.as_secs() as f64 * 1e3) + (duration.subsec_nanos() as f64 / 1e6)
}
