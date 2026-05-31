use std::{path::Path, process::Command};

use anyhow::{Context, bail};

// Returns the actual allocated disk usage of an overlay image in MB.
// Uses `du --block-size=1M` which counts real blocks, not logical sparse size.
// Returns 0 if the path doesn't exist or du fails.
pub fn measure_overlay_usage_mb(path: &Path) -> i32 {
    let output = Command::new("du")
        .args(["--block-size=1M", "--summarize", &path.to_string_lossy()])
        .output();

    let Ok(output) = output else { return 0 };
    if !output.status.success() {
        return 0;
    }

    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0)
}

pub fn provision_overlay(path: &Path, size_mb: u64) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create overlay dir: {}", parent.display()))?;
    }

    let size_bytes = size_mb * 1024 * 1024;

    let status = Command::new("fallocate")
        .args([
            "-l",
            &size_bytes.to_string(),
            path.to_string_lossy().as_ref(),
        ])
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

/// Grows an existing ext4 overlay to `new_size_mb`. Runs e2fsck then resize2fs.
/// No-ops if the file is already at least `new_size_mb`.
pub fn resize_overlay(path: &Path, new_size_mb: u64) -> anyhow::Result<()> {
    let current_bytes = std::fs::metadata(path)
        .map(|m| m.len())
        .unwrap_or(0);
    let new_bytes = new_size_mb * 1024 * 1024;

    if current_bytes >= new_bytes {
        return Ok(());
    }

    // Extend the file to the new size.
    let status = Command::new("fallocate")
        .args(["-l", &new_bytes.to_string(), path.to_string_lossy().as_ref()])
        .status()
        .context("run fallocate for resize")?;
    if !status.success() {
        bail!("fallocate failed while resizing {}", path.display());
    }

    // e2fsck required before resize2fs on an offline filesystem.
    let _ = Command::new("e2fsck")
        .args(["-f", "-p", path.to_string_lossy().as_ref()])
        .status();

    let status = Command::new("resize2fs")
        .arg(path)
        .status()
        .context("run resize2fs")?;
    if !status.success() {
        bail!("resize2fs failed for {}", path.display());
    }

    Ok(())
}

pub fn remove_overlay(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::measure_overlay_usage_mb;

    #[test]
    fn measure_returns_zero_for_nonexistent_path() {
        let path = std::path::Path::new("/tmp/spwn-test-nonexistent-overlay.ext4");
        assert_eq!(measure_overlay_usage_mb(path), 0);
    }

    #[test]
    fn measure_returns_nonzero_for_real_file() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        // Write 2 MB of actual data so du reports >= 1 MB.
        let buf = vec![0xABu8; 2 * 1024 * 1024];
        f.write_all(&buf).unwrap();
        f.flush().unwrap();

        let usage = measure_overlay_usage_mb(f.path());
        assert!(usage >= 1, "expected >= 1 MB usage, got {usage}");
    }

    #[test]
    fn measure_returns_zero_for_empty_file() {
        let f = tempfile::NamedTempFile::new().unwrap();
        // An empty file has 0 allocated blocks; du rounds to 0 with --block-size=1M.
        let usage = measure_overlay_usage_mb(f.path());
        assert_eq!(usage, 0);
    }
}
