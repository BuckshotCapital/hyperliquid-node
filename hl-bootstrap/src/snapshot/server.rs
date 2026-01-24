use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use axum::Json;
use axum::body::Body;
use axum::extract::Query;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Router, extract::State};
use reqwest::{Client, ClientBuilder, StatusCode};
use serde::Deserialize;
use serde_json::json;
use tokio::fs::File;
use tokio::net::TcpListener;
use tokio_util::io::ReaderStream;

use crate::axum_ext::HttpResult;

static CLIENT: LazyLock<Client> = LazyLock::new(|| ClientBuilder::new().build().unwrap());

#[derive(Clone)]
struct SnapshotServer {
    snapshot_directory: PathBuf,
}

fn router() -> Router<SnapshotServer> {
    Router::new().route("/snapshot", get(snapshot))
}

#[derive(Debug, Deserialize)]
struct SnapshotRequest {
    #[serde(flatten)]
    snapshot: super::FileSnapshotType,

    #[serde(
        rename = "includeHeightInOutput",
        default = "default_include_height_in_output"
    )]
    include_height_in_output: bool,

    #[serde(rename = "streamContents", default)]
    stream_contents: bool,
}

fn default_include_height_in_output() -> bool {
    true
}

async fn snapshot(
    State(state): State<SnapshotServer>,
    Query(SnapshotRequest {
        snapshot,
        include_height_in_output,
        stream_contents,
    }): Query<SnapshotRequest>,
) -> HttpResult<impl IntoResponse> {
    let snapshot_path = super::create_file_snapshot_path(&state.snapshot_directory, &snapshot);
    let payload =
        super::create_file_snapshot_payload(&snapshot, include_height_in_output, &snapshot_path);

    let _response = CLIENT
        .post("http://127.0.0.1:3001/info")
        .json(&payload)
        .send()
        .await?
        .error_for_status()?;

    // TODO: assert response

    if !stream_contents {
        return Ok((
            StatusCode::OK,
            [(CONTENT_TYPE, "application/json")],
            Json(json!({
                "path": snapshot_path.to_string_lossy(),
            })),
        )
            .into_response());
    }

    let stream = ReaderStream::new(File::open(snapshot_path).await?);

    Ok((
        StatusCode::OK,
        [(CONTENT_TYPE, "application/json")],
        Body::from_stream(stream),
    )
        .into_response())
}

pub async fn run_snapshot_server(
    snapshot_directory: impl AsRef<Path>,
    listen_address: SocketAddr,
) -> eyre::Result<()> {
    let state = SnapshotServer {
        snapshot_directory: snapshot_directory.as_ref().into(),
    };

    let listener = TcpListener::bind(listen_address).await?;
    axum::serve(listener, router().with_state(state).into_make_service()).await?;

    Ok(())
}
