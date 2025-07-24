use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::time::{MissedTickBehavior, interval};
use tracing::{info, trace, warn};

/// Worker task that periodically cleans up old files in ${base}/hl/data
/// Equivalent to: find ${base}/hl/data -mindepth 1 -depth -mmin +240 -type f -not -name "visor_child_stderr"
pub async fn prune_worker_task<P: AsRef<Path>>(
    base_path: P,
    prune_interval: Duration,
    prune_older_than: Duration,
) {
    let base_path = base_path.as_ref().join("hl/data");

    let mut interval = interval(prune_interval);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval.tick().await; // will complete immediately, as per interval API

    info!(?base_path, ?prune_older_than, "pruning node data directory");
    if let Err(err) = run_cleanup(&base_path, prune_older_than).await {
        warn!(?err, "initial node data prune failed");
    }

    loop {
        interval.tick().await;

        if let Err(err) = run_cleanup(&base_path, prune_older_than).await {
            warn!(?err, ?prune_older_than, "scheduled node data prune failed");
        }
    }
}

async fn run_cleanup<P: AsRef<Path>>(data_path: P, prune_older_than: Duration) -> eyre::Result<()> {
    let data_path = data_path.as_ref();
    let now = SystemTime::now();

    let mut files_to_remove = Vec::new();

    // Walk directory tree depth-first (equivalent to -depth flag)
    collect_files_recursive(
        data_path,
        data_path,
        &mut files_to_remove,
        prune_older_than,
        now,
    )
    .await?;

    let mut removed = 0_usize;
    let mut failed = 0_usize;

    for file_path in files_to_remove {
        match fs::remove_file(&file_path) {
            Ok(()) => {
                trace!(?file_path, "file removed");
                removed += 1;
            }
            Err(err) => {
                warn!(?err, ?file_path, "failed to remove file");
                failed += 1;
            }
        }
    }

    info!(removed, failed, "prune complete",);

    Ok(())
}

async fn collect_files_recursive(
    current_path: &Path,
    base_path: &Path,
    files_to_remove: &mut Vec<PathBuf>,
    cutoff_duration: Duration,
    now: SystemTime,
) -> eyre::Result<()> {
    let entries = match fs::read_dir(current_path) {
        Ok(entries) => entries,
        Err(err) => {
            warn!(?err, ?current_path, "failed to read directory");
            return Ok(());
        }
    };

    let mut subdirs = Vec::new();

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(err) => {
                warn!(?err, ?path, "failed to get file metadata");
                continue;
            }
        };

        if metadata.is_dir() {
            subdirs.push(path);
        } else if metadata.is_file() {
            if path.parent() == Some(base_path) {
                continue;
            }

            if let Some(filename) = path.file_name() {
                if filename == "visor_child_stderr" {
                    continue;
                }
            }

            if let Ok(modified) = metadata.modified() {
                if let Ok(age) = now.duration_since(modified) {
                    if age > cutoff_duration {
                        files_to_remove.push(path);
                    }
                }
            }
        }
    }

    // Process subdirectories depth-first (equivalent to -depth)
    for subdir in subdirs {
        let task = Box::pin(collect_files_recursive(
            &subdir,
            base_path,
            files_to_remove,
            cutoff_duration,
            now,
        ));
        task.await?;
    }

    Ok(())
}
