use anyhow::Context as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use russh::Disconnect;
use russh::client::{self, Config as SshConfig};
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey, PrivateKeyWithHashAlg, load_secret_key};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tonic::{Request, Response, Status, Streaming};

use agent_proto::agent::{
    AgentEvent, BuildImageEvent, BuildImageRequest, CloneVmRequest, CloneVmResponse, ConsoleInput,
    ConsoleOutput, CreateVmRequest, CreateVmResponse, DeleteVmRequest, DeleteVmResponse,
    MigrateVmRequest, MigrateVmResponse, ResizeBandwidthRequest, ResizeBandwidthResponse,
    ResizeCpuRequest, ResizeCpuResponse, RestoreRequest, RestoreResponse, StartVmRequest,
    StartVmResponse, StopVmRequest, StopVmResponse, TakeSnapshotRequest, TakeSnapshotResponse,
    WatchRequest, build_image_event::Stage, host_agent_server::HostAgent,
};

use crate::manager::{VmEvent, VmManager};

// ── Image build helpers ───────────────────────────────────────────────────────

const ROOTFS_CONFIG: &[u8] = include_bytes!("../../../scripts/rootfs-config.sh");

const OVERLAY_INIT: &str = r#"#!/bin/sh
set -e

REAL_INIT=/sbin/init

mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true

for x in $(cat /proc/cmdline); do
    case "$x" in
    overlay_root=*)  OVERLAY_ROOT="${x#overlay_root=}" ;;
    real_init=*)     REAL_INIT="${x#real_init=}" ;;
    esac
done

if [ -z "$OVERLAY_ROOT" ]; then
    exec "$REAL_INIT" "$@"
fi

mkdir -p /overlay
mount -t ext4 "/dev/${OVERLAY_ROOT}" /overlay

mkdir -p /overlay/upper /overlay/work

mkdir -p /rom
mount -t overlay overlay \
    -o "lowerdir=/,upperdir=/overlay/upper,workdir=/overlay/work" \
    /rom

for dir in dev proc sys; do
    mkdir -p "/rom/${dir}"
    mount --move "/${dir}" "/rom/${dir}" 2>/dev/null || true
done

cd /rom
pivot_root . overlay
exec chroot . "$REAL_INIT" "$@" <dev/console >dev/console 2>&1
"#;

fn event(stage: Stage, message: impl Into<String>) -> Result<BuildImageEvent, Status> {
    Ok(BuildImageEvent {
        stage: stage as i32,
        message: message.into(),
        size_bytes: 0,
    })
}

fn event_with_size(
    stage: Stage,
    message: impl Into<String>,
    size_bytes: i64,
) -> Result<BuildImageEvent, Status> {
    Ok(BuildImageEvent {
        stage: stage as i32,
        message: message.into(),
        size_bytes,
    })
}

fn event_done(size_bytes: i64) -> Result<BuildImageEvent, Status> {
    Ok(BuildImageEvent {
        stage: Stage::Done as i32,
        message: "done".into(),
        size_bytes,
    })
}

fn event_error(msg: impl Into<String>) -> Result<BuildImageEvent, Status> {
    Ok(BuildImageEvent {
        stage: Stage::Error as i32,
        message: msg.into(),
        size_bytes: 0,
    })
}

async fn run_cmd(prog: &str, args: &[&str]) -> anyhow::Result<()> {
    let out = tokio::process::Command::new(prog)
        .args(args)
        .output()
        .await
        .with_context(|| format!("spawn {prog}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            anyhow::bail!("{prog} exited with status {}", out.status);
        } else {
            anyhow::bail!("{prog} exited with status {}: {stderr}", out.status);
        }
    }
    Ok(())
}

/// Detect whether docker or podman is available.
fn container_bin() -> anyhow::Result<&'static str> {
    for bin in ["docker", "podman"] {
        if std::process::Command::new("which")
            .arg(bin)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Ok(bin);
        }
    }
    anyhow::bail!("docker or podman is required to build images")
}

async fn build_image_inner(
    req: BuildImageRequest,
    images_dir: PathBuf,
    tx: tokio::sync::mpsc::Sender<Result<BuildImageEvent, Status>>,
) {
    let send = |ev: Result<BuildImageEvent, Status>| {
        let tx = tx.clone();
        async move { tx.send(ev).await.ok() }
    };

    let bin = match container_bin() {
        Ok(b) => b,
        Err(e) => {
            send(event_error(e.to_string())).await;
            return;
        }
    };

    // ── pull ──────────────────────────────────────────────────────────────────
    tracing::info!(source = %req.source, image_id = %req.image_id, "build_image: pulling");
    send(event(Stage::Pulling, format!("pulling {}", req.source))).await;
    if let Err(e) = run_cmd(bin, &["pull", &req.source]).await {
        tracing::error!(source = %req.source, "build_image: pull failed: {e}");
        send(event_error(format!("pull failed: {e}"))).await;
        return;
    }
    tracing::info!(source = %req.source, "build_image: pull complete");

    // ── export ────────────────────────────────────────────────────────────────
    tracing::info!(source = %req.source, "build_image: exporting container filesystem");
    send(event(Stage::Exporting, format!("exporting {}", req.source))).await;

    let tmpdir = match tempfile::TempDir::new() {
        Ok(d) => d,
        Err(e) => {
            send(event_error(format!("tempdir: {e}"))).await;
            return;
        }
    };
    let rootfs = tmpdir.path().join("rootfs");
    if let Err(e) = tokio::fs::create_dir_all(&rootfs).await {
        send(event_error(format!("mkdir rootfs: {e}"))).await;
        return;
    }

    let container_id = match tokio::process::Command::new(bin)
        .args(["create", &req.source])
        .output()
        .await
    {
        Ok(o) if o.status.success() => {
            let id = String::from_utf8_lossy(&o.stdout).trim().to_string();
            tracing::info!(container_id = %id, "build_image: container created");
            id
        }
        Ok(o) => {
            let msg = format!(
                "container create failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            tracing::error!("build_image: {msg}");
            send(event_error(msg)).await;
            return;
        }
        Err(e) => {
            tracing::error!("build_image: container create: {e}");
            send(event_error(format!("container create: {e}"))).await;
            return;
        }
    };

    // Export the container filesystem and pipe it directly into tar.
    // We spawn both processes manually so we can stream between them without
    // buffering the whole image in memory, and capture each process's stderr
    // independently — a shell pipeline masks docker errors because sh reports
    // tar's exit code (0) even when docker fails.
    let mut docker_child = match tokio::process::Command::new(bin)
        .args(["export", &container_id])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = run_cmd(bin, &["rm", &container_id]).await;
            send(event_error(format!("spawn docker export: {e}"))).await;
            return;
        }
    };

    let mut tar_child = match tokio::process::Command::new("tar")
        .args(["-x", "-C", rootfs.to_str().unwrap()])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = docker_child.kill().await;
            let _ = run_cmd(bin, &["rm", &container_id]).await;
            send(event_error(format!("spawn tar: {e}"))).await;
            return;
        }
    };

    // Stream docker stdout → tar stdin.
    let mut docker_stdout = docker_child.stdout.take().unwrap();
    let mut tar_stdin = tar_child.stdin.take().unwrap();
    if let Err(e) = tokio::io::copy(&mut docker_stdout, &mut tar_stdin).await {
        let _ = docker_child.kill().await;
        let _ = tar_child.kill().await;
        let _ = run_cmd(bin, &["rm", &container_id]).await;
        send(event_error(format!("streaming export to tar: {e}"))).await;
        return;
    }
    // Close tar's stdin so it knows the stream is done.
    drop(tar_stdin);

    let docker_out = docker_child.wait_with_output().await;
    let tar_out = tar_child.wait_with_output().await;

    let _ = run_cmd(bin, &["rm", &container_id]).await;

    match docker_out {
        Ok(o) if o.status.success() => {
            tracing::info!("build_image: docker export succeeded");
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let msg = format!("docker export failed ({}): {}", o.status, stderr.trim());
            tracing::error!("build_image: {msg}");
            send(event_error(msg)).await;
            return;
        }
        Err(e) => {
            tracing::error!("build_image: docker export: {e}");
            send(event_error(format!("docker export: {e}"))).await;
            return;
        }
    }

    match tar_out {
        Ok(o) if o.status.success() => {
            tracing::info!("build_image: tar extraction succeeded");
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let msg = format!("tar extract failed ({}): {}", o.status, stderr.trim());
            tracing::error!("build_image: {msg}");
            send(event_error(msg)).await;
            return;
        }
        Err(e) => {
            tracing::error!("build_image: tar extract: {e}");
            send(event_error(format!("tar extract: {e}"))).await;
            return;
        }
    }

    // Sanity-check: measure the extracted rootfs and bail early if it looks
    // suspiciously small (< 1 MB suggests docker export produced nothing).
    let rootfs_size = tokio::process::Command::new("du")
        .args(["-sb", rootfs.to_str().unwrap()])
        .output()
        .await
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<u64>().ok())
        })
        .unwrap_or(0);

    let rootfs_size_mb = rootfs_size / 1_000_000;
    tracing::info!(
        rootfs_bytes = rootfs_size,
        rootfs_mb = rootfs_size_mb,
        "build_image: rootfs extracted"
    );
    send(event_with_size(
        Stage::Exporting,
        format!("extracted rootfs: {rootfs_size_mb} MB"),
        rootfs_size as i64,
    ))
    .await;

    if rootfs_size < 1_000_000 {
        let msg = format!(
            "extracted rootfs is suspiciously small ({rootfs_size} bytes) — docker export may have failed silently"
        );
        tracing::error!("build_image: {msg}");
        send(event_error(msg)).await;
        return;
    }

    // ── bake in overlay-init, resolv.conf, CA bundle ─────────────────────────
    tracing::info!("build_image: baking overlay-init and config into rootfs");
    send(event(Stage::Exporting, "baking overlay-init into rootfs")).await;

    // Create /overlay and /rom unconditionally — these must exist as real dirs
    // in the squashfs since the rootfs is mounted read-only at boot.
    for dir in &["overlay", "rom"] {
        if let Err(e) = tokio::fs::create_dir_all(rootfs.join(dir)).await {
            send(event_error(format!("mkdir {dir}: {e}"))).await;
            return;
        }
    }

    // Resolve /sbin without creating a real directory — on usrmerge distros
    // (Ubuntu 22.04+) /sbin is a symlink to usr/sbin. We must follow it rather
    // than creating a real /sbin directory, which would shadow the symlink in
    // the squashfs and leave /sbin empty at boot.
    let sbin_link = rootfs.join("sbin");
    let sbin = match tokio::fs::canonicalize(&sbin_link).await {
        Ok(real) => {
            tracing::info!(resolved = %real.display(), "build_image: /sbin resolved (usrmerge distro)");
            real
        }
        Err(_) => {
            tracing::info!("build_image: /sbin not found, creating as real directory");
            if let Err(e) = tokio::fs::create_dir_all(&sbin_link).await {
                send(event_error(format!("mkdir sbin: {e}"))).await;
                return;
            }
            sbin_link
        }
    };
    let overlay_init_path = sbin.join("overlay-init");
    if let Err(e) = tokio::fs::write(&overlay_init_path, OVERLAY_INIT).await {
        send(event_error(format!("write overlay-init: {e}"))).await;
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ =
            tokio::fs::set_permissions(&overlay_init_path, std::fs::Permissions::from_mode(0o755))
                .await;
    }

    // Resolve the actual in-guest path to overlay-init, accounting for usrmerge
    // distros (Ubuntu 22.04+) where /sbin is a symlink to usr/sbin. We follow
    // the symlink on the extracted rootfs to find the canonical host path, then
    // strip the rootfs prefix to get the absolute guest path. This is written to
    // a .overlay-init sidecar so the boot args use the right init= path.
    let overlay_init_guest_path = match std::fs::canonicalize(&overlay_init_path) {
        Ok(canonical) => canonical
            .strip_prefix(&rootfs)
            .map(|rel| format!("/{}", rel.to_string_lossy()))
            .unwrap_or_else(|_| "/sbin/overlay-init".to_string()),
        Err(_) => "/sbin/overlay-init".to_string(),
    };
    tracing::info!(init_path = %overlay_init_guest_path, "build_image: overlay-init guest path");
    send(event(
        Stage::Exporting,
        format!("overlay-init path: {overlay_init_guest_path}"),
    ))
    .await;
    let sidecar_path = images_dir.join(format!("{}.overlay-init", req.image_id));
    let _ = tokio::fs::write(&sidecar_path, &overlay_init_guest_path).await;

    // resolv.conf
    let _ = tokio::fs::write(
        rootfs.join("etc/resolv.conf"),
        "nameserver 8.8.8.8\nnameserver 1.1.1.1\n",
    )
    .await;

    // CA bundle — copy from host
    let ca_candidates = [
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/tls/certs/ca-bundle.crt",
        "/etc/ssl/cert.pem",
    ];
    let mut ca_injected = false;
    for candidate in &ca_candidates {
        if tokio::fs::metadata(candidate).await.is_ok() {
            let dest = rootfs.join("etc/ssl/certs/ca-certificates.crt");
            if let Some(parent) = dest.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            let _ = tokio::fs::copy(candidate, &dest).await;
            tracing::info!(source = candidate, "build_image: CA bundle injected");
            ca_injected = true;
            break;
        }
    }
    if !ca_injected {
        tracing::warn!("build_image: no CA bundle found on host — TLS may fail inside the VM");
    }

    // ── chroot: install init system + openssh + basic tools ──────────────────
    // The base docker image for many distros ships without an init system.
    // Without one, overlay-init's exec of REAL_INIT fails immediately and the
    // VM dies. We embed rootfs-config.sh and run it via chroot so distro
    // detection and package install live in one place (the shell script).
    tracing::info!("build_image: running rootfs-config in chroot");
    send(event(
        Stage::Exporting,
        "installing packages (systemd, openssh-server, ...)",
    ))
    .await;

    let config_script_path = rootfs.join("tmp/rootfs-config.sh");
    if let Err(e) = tokio::fs::write(&config_script_path, ROOTFS_CONFIG).await {
        send(event_error(format!("write rootfs-config.sh: {e}"))).await;
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ =
            tokio::fs::set_permissions(&config_script_path, std::fs::Permissions::from_mode(0o755))
                .await;
    }

    // Bind-mount proc/sys/dev so package managers work correctly inside the chroot.
    let mounts: &[(&str, &str, Option<&str>)] = &[
        ("proc", "proc", Some("proc")),
        ("sysfs", "sys", Some("sysfs")),
        ("/dev", "dev", None),
        ("/dev/pts", "dev/pts", None),
    ];
    let mut mounted = vec![];
    for (src, rel_dst, fstype) in mounts {
        let dst = rootfs.join(rel_dst);
        let _ = tokio::fs::create_dir_all(&dst).await;
        let mut cmd = tokio::process::Command::new("mount");
        if let Some(t) = fstype {
            cmd.args(["-t", t, src]);
        } else {
            cmd.args(["--bind", src]);
        }
        cmd.arg(dst.to_str().unwrap());
        match cmd.status().await {
            Ok(s) if s.success() => mounted.push(dst),
            Ok(s) => tracing::warn!("build_image: mount {src} failed: {s}"),
            Err(e) => tracing::warn!("build_image: mount {src}: {e}"),
        }
    }

    let chroot_out = tokio::process::Command::new("chroot")
        .arg(rootfs.to_str().unwrap())
        .args(["/tmp/rootfs-config.sh"])
        .output()
        .await;

    // Unmount in reverse order regardless of outcome.
    for dst in mounted.into_iter().rev() {
        let _ = tokio::process::Command::new("umount")
            .arg(dst.to_str().unwrap())
            .status()
            .await;
    }
    let _ = tokio::fs::remove_file(&config_script_path).await;

    match chroot_out {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stdout = stdout.trim();
            if !stdout.is_empty() {
                tracing::info!("build_image: rootfs-config output:\n{stdout}");
            }
            tracing::info!("build_image: package install complete");
            send(event(Stage::Exporting, "package install complete")).await;
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let stdout = String::from_utf8_lossy(&o.stdout);
            let detail = format!("{}\n{}", stdout.trim(), stderr.trim());
            let msg = format!("rootfs-config failed ({}): {}", o.status, detail.trim());
            tracing::error!("build_image: {msg}");
            send(event_error(msg)).await;
            return;
        }
        Err(e) => {
            tracing::error!("build_image: spawn chroot rootfs-config: {e}");
            send(event_error(format!("spawn chroot rootfs-config: {e}"))).await;
            return;
        }
    }

    // ── platform SSH public key ───────────────────────────────────────────────
    match inject_platform_pubkey(&rootfs).await {
        Ok(true) => {
            tracing::info!("build_image: platform SSH public key injected");
            send(event(Stage::Exporting, "platform SSH key injected")).await;
        }
        Ok(false) => {
            tracing::warn!(
                "build_image: platform key not found — image will require manual authorized_keys setup"
            );
            send(event(
                Stage::Exporting,
                "warning: platform SSH key not found, skipping injection",
            ))
            .await;
        }
        Err(e) => {
            tracing::warn!("build_image: failed to inject platform key: {e}");
            send(event(
                Stage::Exporting,
                format!("warning: platform key injection failed: {e}"),
            ))
            .await;
        }
    }

    // ── squash ────────────────────────────────────────────────────────────────
    tracing::info!(output = %images_dir.join(format!("{}.sqfs", req.image_id)).display(), "build_image: running mksquashfs");
    send(event(Stage::Squashing, "building squashfs")).await;

    let output_path = images_dir.join(format!("{}.sqfs", req.image_id));
    let mksquashfs_out = tokio::process::Command::new("mksquashfs")
        .args([
            rootfs.to_str().unwrap(),
            output_path.to_str().unwrap(),
            "-noappend",
            "-comp",
            "zstd",
        ])
        .output()
        .await;

    match mksquashfs_out {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stdout = stdout.trim();
            if !stdout.is_empty() {
                tracing::info!("build_image: mksquashfs output:\n{stdout}");
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let stdout = String::from_utf8_lossy(&o.stdout);
            let detail = format!("{}\n{}", stdout.trim(), stderr.trim());
            let detail = detail.trim();
            let msg = format!("mksquashfs failed ({}): {detail}", o.status);
            tracing::error!("build_image: {msg}");
            send(event_error(msg)).await;
            return;
        }
        Err(e) => {
            tracing::error!("build_image: spawn mksquashfs: {e}");
            send(event_error(format!("spawn mksquashfs: {e}"))).await;
            return;
        }
    }

    let size_bytes = tokio::fs::metadata(&output_path)
        .await
        .map(|m| m.len() as i64)
        .unwrap_or(0);

    let size_mb = size_bytes / 1_000_000;
    let ratio = if rootfs_size > 0 {
        (size_bytes as f64 / rootfs_size as f64) * 100.0
    } else {
        0.0
    };
    tracing::info!(
        size_bytes,
        size_mb,
        rootfs_bytes = rootfs_size,
        compression_pct = format!("{ratio:.1}%"),
        "build_image: squashfs complete"
    );
    send(event_with_size(
        Stage::Squashing,
        format!("squashfs: {size_mb} MB ({ratio:.1}% of extracted rootfs)"),
        size_bytes,
    ))
    .await;

    // ── sanity check: verify init binary exists in the rootfs ─────────────────
    // Read the overlay-init guest path we resolved earlier to find REAL_INIT.
    // overlay-init defaults to /sbin/init; check that path exists in the rootfs
    // so we catch a missing init before the image is marked ready.
    let init_candidates = [
        "sbin/init",
        "usr/sbin/init",
        "usr/lib/systemd/systemd",
        "bin/sh",
        "usr/bin/sh",
        "bin/bash",
        "usr/bin/bash",
    ];
    let init_found = init_candidates.iter().any(|p| rootfs.join(p).exists());

    if !init_found {
        let msg = format!(
            "no init binary found in rootfs (checked: {}); \
             the VM will crash immediately after boot — \
             ensure the image has systemd or a shell installed",
            init_candidates.join(", ")
        );
        tracing::error!("build_image: {msg}");
        send(event_error(msg)).await;
        return;
    }

    let init_path = init_candidates
        .iter()
        .find(|p| rootfs.join(p).exists())
        .unwrap();
    tracing::info!(init_path, "build_image: init binary present");
    send(event(
        Stage::Squashing,
        format!("init binary found: /{init_path}"),
    ))
    .await;

    send(event_done(size_bytes)).await;
}

// ── Platform SSH key ──────────────────────────────────────────────────────────

/// Inject the platform SSH public key into `rootfs/root/.ssh/authorized_keys`.
/// Returns `Ok(true)` if the key was present and injected, `Ok(false)` if the
/// platform key file doesn't exist yet (agent hasn't generated it).
async fn inject_platform_pubkey(rootfs: &std::path::Path) -> anyhow::Result<bool> {
    let key_path = platform_key_path();
    if !key_path.exists() {
        return Ok(false);
    }

    let private_key = load_or_generate_platform_key()?;
    let pubkey = private_key.public_key();
    let pubkey_str = pubkey
        .to_openssh()
        .map_err(|e| anyhow::anyhow!("serialize pubkey: {e}"))?;

    let ssh_dir = rootfs.join("root/.ssh");
    tokio::fs::create_dir_all(&ssh_dir).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(&ssh_dir, std::fs::Permissions::from_mode(0o700)).await;
    }

    let authorized_keys_path = ssh_dir.join("authorized_keys");
    let existing = tokio::fs::read_to_string(&authorized_keys_path)
        .await
        .unwrap_or_default();

    if !existing.contains(pubkey_str.trim()) {
        let mut content = existing;
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(pubkey_str.trim());
        content.push('\n');
        tokio::fs::write(&authorized_keys_path, &content).await?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(
            &authorized_keys_path,
            std::fs::Permissions::from_mode(0o600),
        )
        .await;
    }

    Ok(true)
}

struct SshClientHandler;

impl client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true) // trust our own VMs; host key verified by network isolation
    }
}

fn platform_key_path() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("PLATFORM_KEY_PATH").unwrap_or_else(|_| "/var/lib/spwn/platform_key".into()),
    )
}

fn load_or_generate_platform_key() -> anyhow::Result<PrivateKey> {
    use std::os::unix::fs::PermissionsExt;

    let path = platform_key_path();
    if path.exists() {
        return load_secret_key(&path, None).map_err(|e| anyhow::anyhow!("load key: {e}"));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let key = PrivateKey::random(&mut rand::rngs::OsRng, Algorithm::Ed25519)
        .map_err(|e| anyhow::anyhow!("generate key: {e}"))?;
    key.write_openssh_file(&path, LineEnding::LF)
        .map_err(|e| anyhow::anyhow!("write key: {e}"))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    tracing::info!(
        path = %path.display(),
        pubkey = %key.public_key().to_openssh().unwrap_or_default(),
        "generated platform SSH key — add the public key to rootfs /root/.ssh/authorized_keys"
    );
    Ok(key)
}

pub struct HostAgentService {
    pub manager: Arc<VmManager>,
    pub agent_secret: String,
}

#[tonic::async_trait]
impl HostAgent for HostAgentService {
    async fn create_vm(
        &self,
        req: Request<CreateVmRequest>,
    ) -> Result<Response<CreateVmResponse>, Status> {
        let r = req.into_inner();
        match self
            .manager
            .create_vm(
                &r.vm_id,
                &r.account_id,
                &r.name,
                &r.subdomain,
                &r.image,
                r.vcpus,
                r.memory_mb,
                r.disk_mb,
                r.bandwidth_mbps,
                r.exposed_port,
                &r.ip_address,
            )
            .await
        {
            Ok(()) => Ok(Response::new(CreateVmResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(CreateVmResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn start_vm(
        &self,
        req: Request<StartVmRequest>,
    ) -> Result<Response<StartVmResponse>, Status> {
        let vm_id = req.into_inner().vm_id;
        match self.manager.start_vm(&vm_id).await {
            Ok(()) => Ok(Response::new(StartVmResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(StartVmResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn stop_vm(
        &self,
        req: Request<StopVmRequest>,
    ) -> Result<Response<StopVmResponse>, Status> {
        let vm_id = req.into_inner().vm_id;
        match self.manager.stop_vm(&vm_id).await {
            Ok(()) => Ok(Response::new(StopVmResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(StopVmResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn delete_vm(
        &self,
        req: Request<DeleteVmRequest>,
    ) -> Result<Response<DeleteVmResponse>, Status> {
        let vm_id = req.into_inner().vm_id;
        match self.manager.delete_vm(&vm_id).await {
            Ok(()) => Ok(Response::new(DeleteVmResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(DeleteVmResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn take_snapshot(
        &self,
        req: Request<TakeSnapshotRequest>,
    ) -> Result<Response<TakeSnapshotResponse>, Status> {
        let r = req.into_inner();
        let label = if r.label.is_empty() {
            None
        } else {
            Some(r.label)
        };
        match self.manager.take_snapshot(&r.vm_id, label).await {
            Ok(snap) => Ok(Response::new(TakeSnapshotResponse {
                ok: true,
                error: String::new(),
                snap_id: snap.id,
                size_bytes: snap.size_bytes,
            })),
            Err(e) => Ok(Response::new(TakeSnapshotResponse {
                ok: false,
                error: format!("{e:#}"),
                snap_id: String::new(),
                size_bytes: 0,
            })),
        }
    }

    async fn restore_snapshot(
        &self,
        req: Request<RestoreRequest>,
    ) -> Result<Response<RestoreResponse>, Status> {
        let r = req.into_inner();
        match self.manager.restore_snapshot(&r.vm_id, &r.snap_id).await {
            Ok(()) => Ok(Response::new(RestoreResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(RestoreResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn clone_vm(
        &self,
        req: Request<CloneVmRequest>,
    ) -> Result<Response<CloneVmResponse>, Status> {
        let r = req.into_inner();
        match self
            .manager
            .clone_vm(
                &r.source_vm_id,
                &r.new_vm_id,
                &r.account_id,
                &r.name,
                &r.subdomain,
                &r.ip_address,
                r.exposed_port,
                r.include_memory,
            )
            .await
        {
            Ok(()) => Ok(Response::new(CloneVmResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(CloneVmResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn migrate_vm(
        &self,
        req: Request<MigrateVmRequest>,
    ) -> Result<Response<MigrateVmResponse>, Status> {
        let r = req.into_inner();
        let secret = self.agent_secret.clone();
        match self
            .manager
            .migrate_vm(
                &r.vm_id,
                &r.source_snapshot_url,
                &r.account_id,
                &r.name,
                &r.subdomain,
                r.vcpus,
                r.memory_mb,
                r.disk_mb,
                r.bandwidth_mbps,
                &r.ip_address,
                r.exposed_port,
                &r.image,
                &secret,
            )
            .await
        {
            Ok(()) => Ok(Response::new(MigrateVmResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(MigrateVmResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn resize_cpu(
        &self,
        req: Request<ResizeCpuRequest>,
    ) -> Result<Response<ResizeCpuResponse>, Status> {
        let r = req.into_inner();
        match self.manager.resize_cpu(&r.vm_id, r.vcpus).await {
            Ok(()) => Ok(Response::new(ResizeCpuResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(ResizeCpuResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    async fn resize_bandwidth(
        &self,
        req: Request<ResizeBandwidthRequest>,
    ) -> Result<Response<ResizeBandwidthResponse>, Status> {
        let r = req.into_inner();
        match self
            .manager
            .resize_bandwidth(&r.vm_id, r.bandwidth_mbps)
            .await
        {
            Ok(()) => Ok(Response::new(ResizeBandwidthResponse {
                ok: true,
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(ResizeBandwidthResponse {
                ok: false,
                error: format!("{e:#}"),
            })),
        }
    }

    type BuildImageStream = std::pin::Pin<
        Box<dyn futures_core::Stream<Item = Result<BuildImageEvent, Status>> + Send + 'static>,
    >;

    async fn build_image(
        &self,
        req: Request<BuildImageRequest>,
    ) -> Result<Response<Self::BuildImageStream>, Status> {
        let req = req.into_inner();
        let images_dir = self.manager.images_dir.clone();

        let (tx, rx) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move {
            build_image_inner(req, images_dir, tx).await;
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream)))
    }

    type WatchEventsStream = std::pin::Pin<
        Box<dyn futures_core::Stream<Item = Result<AgentEvent, Status>> + Send + 'static>,
    >;

    async fn watch_events(
        &self,
        _req: Request<WatchRequest>,
    ) -> Result<Response<Self::WatchEventsStream>, Status> {
        let rx = self.manager.subscribe_events();
        let stream = BroadcastStream::new(rx).filter_map(|result| {
            let event = result.ok()?;
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let proto = match event {
                VmEvent::Started { vm_id } => AgentEvent {
                    vm_id,
                    event: "started".into(),
                    detail: String::new(),
                    timestamp,
                },
                VmEvent::Stopped { vm_id } => AgentEvent {
                    vm_id,
                    event: "stopped".into(),
                    detail: String::new(),
                    timestamp,
                },
                VmEvent::Crashed { vm_id } => AgentEvent {
                    vm_id,
                    event: "crashed".into(),
                    detail: String::new(),
                    timestamp,
                },
                VmEvent::SnapshotTaken { vm_id, snap_id } => AgentEvent {
                    vm_id,
                    event: "snapshot_taken".into(),
                    detail: snap_id,
                    timestamp,
                },
            };
            Some(Ok(proto))
        });

        Ok(Response::new(Box::pin(stream)))
    }

    type StreamConsoleStream = std::pin::Pin<
        Box<dyn futures_core::Stream<Item = Result<ConsoleOutput, Status>> + Send + 'static>,
    >;

    async fn stream_console(
        &self,
        req: Request<Streaming<ConsoleInput>>,
    ) -> Result<Response<Self::StreamConsoleStream>, Status> {
        let mut input_stream = req.into_inner();

        // First frame carries vm_id; subsequent frames carry data.
        let first = input_stream
            .next()
            .await
            .ok_or_else(|| Status::invalid_argument("stream closed before first frame"))?
            .map_err(|e| Status::internal(e.to_string()))?;

        let vm_id = first.vm_id;
        if vm_id.is_empty() {
            return Err(Status::invalid_argument("first frame must set vm_id"));
        }
        let initial_data = first.data;
        let command = first.command;

        let vm = db::get_vm(&self.manager.pool, &vm_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("vm not found"))?;

        if vm.status != "running" {
            return Err(Status::failed_precondition(format!(
                "vm is {} (must be running)",
                vm.status
            )));
        }

        let key = load_or_generate_platform_key()
            .map_err(|e| Status::internal(format!("platform key: {e}")))?;

        let config = Arc::new(SshConfig::default());
        let mut handle = client::connect(config, (vm.ip_address.as_str(), 22u16), SshClientHandler)
            .await
            .map_err(|e| Status::unavailable(format!("ssh connect to vm: {e}")))?;

        let hash_alg = match handle.best_supported_rsa_hash().await {
            Ok(outer) => outer.flatten(),
            Err(_) => None,
        };

        let auth_result = handle
            .authenticate_publickey("root", PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg))
            .await
            .map_err(|e| Status::unauthenticated(format!("ssh auth: {e}")))?;

        if !auth_result.success() {
            return Err(Status::unauthenticated(
                "ssh authentication failed — ensure platform public key is in vm's authorized_keys",
            ));
        }

        let channel = handle
            .channel_open_session()
            .await
            .map_err(|e| Status::internal(format!("open session: {e}")))?;

        if command.is_empty() {
            channel
                .request_pty(false, "xterm-256color", 80, 24, 0, 0, &[])
                .await
                .map_err(|e| Status::internal(format!("pty request: {e}")))?;

            channel
                .request_shell(false)
                .await
                .map_err(|e| Status::internal(format!("shell request: {e}")))?;
        } else {
            channel
                .exec(false, command.as_str())
                .await
                .map_err(|e| Status::internal(format!("exec request: {e}")))?;
        }

        let ssh_stream = channel.into_stream();
        let (mut ssh_reader, mut ssh_writer) = tokio::io::split(ssh_stream);

        let (out_tx, out_rx) = tokio::sync::mpsc::channel::<Result<ConsoleOutput, Status>>(64);
        let out_stream = ReceiverStream::new(out_rx);

        // SSH → gRPC output
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match ssh_reader.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if out_tx
                            .send(Ok(ConsoleOutput {
                                data: buf[..n].to_vec(),
                            }))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            // keep handle alive until SSH reader closes; suppress warning
            let _ = handle.disconnect(Disconnect::ByApplication, "", "").await;
        });

        // gRPC input → SSH
        tokio::spawn(async move {
            if !initial_data.is_empty() {
                let _ = ssh_writer.write_all(&initial_data).await;
            }
            while let Some(Ok(frame)) = input_stream.next().await {
                if frame.data.is_empty() {
                    continue;
                }
                if ssh_writer.write_all(&frame.data).await.is_err() {
                    break;
                }
            }
            let _ = ssh_writer.shutdown().await;
        });

        Ok(Response::new(Box::pin(out_stream)))
    }
}
