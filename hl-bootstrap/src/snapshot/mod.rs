use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_with::{DisplayFromStr, serde_as};
use uuid::Uuid;

pub mod server;

#[serde_as]
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum FileSnapshotType {
    #[serde(rename = "l4Snapshots")]
    L4Snapshots {
        #[serde_as(deserialize_as = "DisplayFromStr")]
        #[serde(rename = "includeUsers", default)]
        include_users: bool,

        #[serde_as(deserialize_as = "DisplayFromStr")]
        #[serde(rename = "includeTriggerOrders", default)]
        include_trigger_orders: bool,
    },
    #[serde(rename = "referrerStates")]
    ReferrerStates,
}

impl FileSnapshotType {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::L4Snapshots { .. } => "l4Snapshots",
            Self::ReferrerStates => "referrerStates",
        }
    }
}

pub fn create_file_snapshot_path(
    base: impl AsRef<Path>,
    snapshot_request: &FileSnapshotType,
) -> PathBuf {
    let snapshot_id = Uuid::now_v7();
    base.as_ref().join(format!(
        "{}_{}.json",
        snapshot_request.type_name(),
        snapshot_id
    ))
}

pub fn create_file_snapshot_payload(
    snapshot_request: &FileSnapshotType,
    include_height_in_output: bool,
    out_path: impl AsRef<Path>,
) -> serde_json::Value {
    json!({
        "type": "fileSnapshot",
        "request": snapshot_request,
        "includeHeightInOutput": include_height_in_output,
        "outPath": out_path.as_ref().to_string_lossy(),
    })
}
