use std::{path::Path, process::Command};

use anyhow::{Context, bail};

pub const DEFAULT_OVERLAY_SIZE_MB: u64 = 5120;

pub fn provision_overlay(path: &Path, size_mb: u64) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create overlay dir: {}", parent.display()))?;
    }

    let size_bytes = size_mb * 1024 * 1024;

    let status = Command::new("fallocate")
        .args(["-l", &size_bytes.to_string(), &path.to_string_lossy().as_ref()])
        .status()
        .context("run fallocate")?;

    if !status.success() {
        bail!("fallocate failed for {}", path.display());
    }

    let status = Command::new("mkfs.ext4")
        .arg("-F")
        .arg(path)
        .status()
        .context("run mkfs.ext4")?;

    if !status.success() {
        let _ = std::fs::remove_file(path);
        bail!("mkfs.ext4 failed for {}", path.display());
    }

    Ok(())
}

pub fn remove_overlay(path: &Path) {
    let _ = std::fs::remove_file(path);
}
