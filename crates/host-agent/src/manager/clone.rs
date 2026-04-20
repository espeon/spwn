use anyhow::{Context, anyhow};
use tracing::info;

use super::{VmManager, util};

impl VmManager {
    pub async fn clone_vm(
        &self,
        source_vm_id: &str,
        new_vm_id: &str,
        account_id: &str,
        name: &str,
        subdomain: &str,
        ip_address: &str,
        exposed_port: i32,
        include_memory: bool,
    ) -> anyhow::Result<()> {
        let source = db::get_vm(&self.pool, source_vm_id)
            .await?
            .ok_or_else(|| anyhow!("source vm not found: {source_vm_id}"))?;

        if matches!(
            source.status.as_str(),
            "starting" | "stopping" | "snapshotting"
        ) {
            return Err(anyhow!(
                "source vm is in transitional state: {}",
                source.status
            ));
        }
        if include_memory && source.status != "running" {
            return Err(anyhow!(
                "include_memory requires source vm to be running (status: {})",
                source.status
            ));
        }

        let source_snap = if include_memory {
            Some(
                self.take_snapshot(source_vm_id, Some(format!("clone-source-{new_vm_id}")))
                    .await
                    .context("take snapshot of source before clone")?,
            )
        } else {
            None
        };

        let source_overlay = source
            .overlay_path
            .as_deref()
            .ok_or_else(|| anyhow!("source vm {source_vm_id} has no overlay"))?;
        let new_overlay_path = self.overlay_dir.join(format!("{new_vm_id}.ext4"));
        copy_sparse(source_overlay, &new_overlay_path)
            .await
            .context("copy overlay")?;

        db::create_vm(
            &self.pool,
            &db::NewVm {
                id: new_vm_id.to_string(),
                account_id: account_id.to_string(),
                name: name.to_string(),
                subdomain: subdomain.to_string(),
                vcpus: source.vcpus,
                memory_mb: source.memory_mb,
                disk_mb: source.disk_mb,
                bandwidth_mbps: source.bandwidth_mbps,
                kernel_path: self.kernel_path.to_string_lossy().into(),
                rootfs_path: source.rootfs_path.clone(),
                overlay_path: new_overlay_path.to_string_lossy().into(),
                real_init: source.real_init.clone(),
                ip_address: ip_address.to_string(),
                exposed_port,
                base_image: source.base_image.clone(),
                cloned_from: Some(source_vm_id.to_string()),
                placement_strategy: source.placement_strategy.clone(),
                required_labels: source.required_labels.clone(),
                region: None,
            },
        )
        .await?;

        let usage = crate::overlay::measure_overlay_usage_mb(&new_overlay_path);
        db::update_disk_usage_mb(&self.pool, new_vm_id, usage)
            .await
            .ok();

        if let Some(snap) = source_snap {
            let new_snap_id = uuid::Uuid::new_v4().to_string();
            let new_snap_path = self.snapshot_dir.join(format!("{new_snap_id}.snap"));
            let new_mem_path = self.snapshot_dir.join(format!("{new_snap_id}.mem"));

            tokio::fs::copy(&snap.snapshot_path, &new_snap_path)
                .await
                .context("copy snapshot file")?;
            tokio::fs::copy(&snap.mem_path, &new_mem_path)
                .await
                .context("copy mem file")?;

            let size_bytes = new_snap_path.metadata().map(|m| m.len()).unwrap_or(0)
                + new_mem_path.metadata().map(|m| m.len()).unwrap_or(0);

            db::create_snapshot(
                &self.pool,
                &db::NewSnapshot {
                    id: new_snap_id.clone(),
                    vm_id: new_vm_id.to_string(),
                    label: Some("cloned".into()),
                    snapshot_path: new_snap_path.to_string_lossy().into(),
                    mem_path: new_mem_path.to_string_lossy().into(),
                    size_bytes: size_bytes as i64,
                },
            )
            .await?;

            self.restore_snapshot(new_vm_id, &new_snap_id).await?;
        }

        info!("cloned vm {source_vm_id} → {new_vm_id} (include_memory={include_memory})");
        Ok(())
    }

    pub async fn migrate_vm(
        &self,
        vm_id: &str,
        source_snapshot_url: &str,
        account_id: &str,
        name: &str,
        subdomain: &str,
        vcpus: i64,
        memory_mb: i32,
        disk_mb: i32,
        bandwidth_mbps: i32,
        ip_address: &str,
        exposed_port: i32,
        image: &str,
        agent_secret: &str,
    ) -> anyhow::Result<()> {
        let rootfs_path = self.images_dir.join(format!("{image}.sqfs"));
        if !rootfs_path.exists() {
            return Err(anyhow!(
                "image '{image}' not found on target host (expected {})",
                rootfs_path.display()
            ));
        }

        let real_init = util::read_image_init(&self.images_dir, image);

        let local_overlay = self.overlay_dir.join(format!("{vm_id}.ext4"));
        download_file(
            &format!("{source_snapshot_url}/overlay/{vm_id}"),
            &local_overlay,
            agent_secret,
        )
        .await
        .context("download overlay from source agent")?;

        db::create_vm(
            &self.pool,
            &db::NewVm {
                id: vm_id.to_string(),
                account_id: account_id.to_string(),
                name: name.to_string(),
                subdomain: subdomain.to_string(),
                vcpus,
                memory_mb,
                disk_mb,
                bandwidth_mbps,
                kernel_path: self.kernel_path.to_string_lossy().into(),
                rootfs_path: rootfs_path.to_string_lossy().into(),
                overlay_path: local_overlay.to_string_lossy().into(),
                real_init,
                ip_address: ip_address.to_string(),
                exposed_port,
                base_image: image.to_string(),
                cloned_from: None,
                placement_strategy: "best_fit".into(),
                required_labels: None,
                region: None,
            },
        )
        .await?;

        let usage = crate::overlay::measure_overlay_usage_mb(&local_overlay);
        db::update_disk_usage_mb(&self.pool, vm_id, usage)
            .await
            .ok();

        info!("migrated vm {vm_id} to this host from {source_snapshot_url}");
        Ok(())
    }
}

async fn copy_sparse(src: &str, dst: &std::path::Path) -> anyhow::Result<()> {
    if let Some(parent) = dst.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let status = tokio::process::Command::new("cp")
        .args(["--sparse=always", src, &dst.to_string_lossy()])
        .status()
        .await
        .context("run cp --sparse=always")?;
    if !status.success() {
        anyhow::bail!("cp --sparse=always failed: {src} -> {}", dst.display());
    }
    Ok(())
}

async fn download_file(
    url: &str,
    dest: &std::path::Path,
    bearer_token: &str,
) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    use tokio_stream::StreamExt;

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let resp = reqwest::Client::new()
        .get(url)
        .bearer_auth(bearer_token)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;

    if !resp.status().is_success() {
        return Err(anyhow!("GET {url}: HTTP {}", resp.status()));
    }

    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("create {}", dest.display()))?;

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("read body from {url}"))?;
        file.write_all(&chunk).await?;
    }

    Ok(())
}
