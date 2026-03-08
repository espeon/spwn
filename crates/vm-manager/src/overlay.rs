use std::{path::Path, process::Command};

use anyhow::{Context, bail};

pub const DEFAULT_OVERLAY_SIZE_MB: u64 = 5120;

/// Provisions a sparse ext4 image at `path` with `size_mb` megabytes of capacity.
///
/// Uses `fallocate` to create a sparse file (much faster than `dd`) then formats
/// it with `mkfs.ext4`. Actual disk usage starts near zero and grows only as the
/// guest writes data into the writable layer.
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

/// Removes the overlay image file. Errors are silently ignored — the overlay
/// file is best-effort cleanup; DB and process state are the source of truth.
pub fn remove_overlay(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provision_creates_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.ext4");

        provision_overlay(&path, 32).expect("provision_overlay should succeed");

        assert!(path.exists(), "overlay file should exist after provisioning");
        assert!(
            std::fs::metadata(&path).expect("metadata").len() > 0,
            "overlay file should be non-empty"
        );
    }

    #[test]
    fn provision_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a/b/c/overlay.ext4");

        provision_overlay(&path, 32).expect("provision_overlay should create nested dirs");

        assert!(path.exists(), "overlay file should exist at nested path");
    }

    #[test]
    fn remove_overlay_deletes_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("remove_me.ext4");

        provision_overlay(&path, 32).expect("provision for remove test");
        assert!(path.exists(), "file should exist before remove");

        remove_overlay(&path);

        assert!(!path.exists(), "overlay file should be gone after remove_overlay");
    }

    #[test]
    fn remove_overlay_is_noop_for_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nonexistent.ext4");
        remove_overlay(&path);
    }

    #[test]
    fn provision_rejects_zero_size() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("zero.ext4");

        let result = provision_overlay(&path, 0);
        assert!(result.is_err(), "zero-size overlay should fail");
    }
}
