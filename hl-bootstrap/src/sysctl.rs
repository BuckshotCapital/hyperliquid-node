use std::{fs, path::PathBuf};

use eyre::Context;

pub fn read_sysctl(key: &str) -> eyre::Result<String> {
    let key_normalized = key.replace('/', ".");
    let path = PathBuf::from("/proc/sys").join(key.replace('.', "/"));
    let value = fs::read_to_string(path)
        .wrap_err_with(|| format!("failed to read sysctl {key_normalized}"))?;

    Ok(value.trim().to_string())
}
