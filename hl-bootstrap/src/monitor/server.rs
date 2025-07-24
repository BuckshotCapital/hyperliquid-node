use std::sync::LazyLock;
use std::{net::SocketAddr, ops::Sub, time::Duration};

use axum::http::HeaderMap;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Router, extract::State};
use prometheus::TextEncoder;
use reqwest::StatusCode;
use tokio::net::TcpListener;

use crate::monitor::{
    GAUGE_HL_NODE_RESPONDING, GAUGE_HL_NODE_SYSTEM_TIME_MS, GAUGE_HL_NODE_TIME_MS, as_ms_f64,
};

#[derive(Clone)]
struct MonitorServer {
    healthy_drift_threshold: Duration,
}

fn router() -> Router<MonitorServer> {
    Router::new()
        .route("/metrics", get(metrics))
        .route("/livez", get(livez))
        .route("/readyz", get(readyz))
}

async fn metrics() -> impl IntoResponse {
    static PROMETHEUS_HEADERS: LazyLock<HeaderMap> = LazyLock::new(|| {
        HeaderMap::from_iter([(CONTENT_TYPE, "text/plain;version=0.0.4".parse().unwrap())])
    });

    let metrics = prometheus::default_registry().gather();

    (
        PROMETHEUS_HEADERS.clone(),
        TextEncoder::new().encode_to_string(&metrics).unwrap(),
    )
}

async fn livez() -> impl IntoResponse {
    if GAUGE_HL_NODE_RESPONDING.get() == 1 {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn readyz(State(state): State<MonitorServer>) -> impl IntoResponse {
    if GAUGE_HL_NODE_RESPONDING.get() == 1
        && GAUGE_HL_NODE_SYSTEM_TIME_MS
            .get()
            .sub(GAUGE_HL_NODE_TIME_MS.get())
            .max(0.0)
            < as_ms_f64(&state.healthy_drift_threshold)
    {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

pub async fn run_metrics_server(
    listen_address: SocketAddr,
    healthy_drift_threshold: Duration,
) -> eyre::Result<()> {
    let state = MonitorServer {
        healthy_drift_threshold,
    };

    let listener = TcpListener::bind(listen_address).await?;
    axum::serve(listener, router().with_state(state).into_make_service()).await?;

    Ok(())
}
