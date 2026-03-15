use std::path::PathBuf;

use tonic::Status;

use agent_proto::agent::{BuildImageEvent, BuildImageRequest, build_image_event::Stage};

use crate::platform_key::inject_platform_pubkey;

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
    hostname=*)      HOSTNAME="${x#hostname=}" ;;
    esac
done

if [ -n "$HOSTNAME" ]; then
    echo "$HOSTNAME" > /proc/sys/kernel/hostname 2>/dev/null || true
fi

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

fn generate_dockerfile(source: &str) -> String {
    format!(
        r#"FROM {source}

RUN if command -v apt-get > /dev/null 2>&1; then \
        apt-get update -qq && \
        DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
            systemd systemd-sysv dbus openssh-server sudo ca-certificates git && \
        apt-get clean && rm -rf /var/lib/apt/lists/*; \
    elif command -v dnf > /dev/null 2>&1; then \
        dnf install -y systemd openssh-server sudo ca-certificates git && dnf clean all; \
    elif command -v yum > /dev/null 2>&1; then \
        yum install -y systemd openssh-server sudo ca-certificates git && yum clean all; \
    elif command -v apk > /dev/null 2>&1; then \
        apk add --no-cache openrc openssh sudo ca-certificates git; \
    else \
        echo "unsupported package manager" >&2 && exit 1; \
    fi

RUN if [ -d /etc/ssh/sshd_config.d ]; then \
        printf 'PermitRootLogin yes\n' > /etc/ssh/sshd_config.d/99-spwn.conf; \
    else \
        sed -i 's/^#*PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config 2>/dev/null || \
        echo 'PermitRootLogin yes' >> /etc/ssh/sshd_config; \
    fi

RUN systemctl enable ssh 2>/dev/null || \
    systemctl enable sshd 2>/dev/null || \
    rc-update add sshd default 2>/dev/null || \
    true

RUN if command -v systemctl > /dev/null 2>&1; then \
        systemctl set-default multi-user.target 2>/dev/null || true; \
        for svc in \
            NetworkManager \
            firewalld \
            systemd-udevd \
            systemd-resolved \
            systemd-networkd \
            systemd-networkd-wait-online \
            ModemManager \
            bluetooth \
            avahi-daemon; \
        do systemctl mask "$svc" 2>/dev/null || true; done; \
    fi

RUN mkdir -p /root/.ssh && chmod 700 /root/.ssh
"#,
        source = source
    )
}

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
    use anyhow::Context as _;
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

pub async fn build_image_inner(
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

    // ── build image via Dockerfile ─────────────────────────────────────────────
    tracing::info!(source = %req.source, image_id = %req.image_id, "build_image: generating Dockerfile");
    send(event(Stage::Pulling, format!("building from {}", req.source))).await;

    let tmpdir = match tempfile::TempDir::new() {
        Ok(d) => d,
        Err(e) => {
            send(event_error(format!("tempdir: {e}"))).await;
            return;
        }
    };

    let dockerfile_path = tmpdir.path().join("Dockerfile");
    let dockerfile = generate_dockerfile(&req.source);
    if let Err(e) = tokio::fs::write(&dockerfile_path, dockerfile).await {
        send(event_error(format!("write Dockerfile: {e}"))).await;
        return;
    }

    let build_tag = format!("spwn-build-{}", req.image_id);
    tracing::info!(source = %req.source, tag = %build_tag, "build_image: running docker build");
    send(event(
        Stage::Pulling,
        format!("building image (installing systemd, openssh-server, ...)"),
    ))
    .await;

    if let Err(e) = run_cmd(
        bin,
        &[
            "build",
            "--tag",
            &build_tag,
            "--network=host",
            "--file",
            dockerfile_path.to_str().unwrap(),
            tmpdir.path().to_str().unwrap(),
        ],
    )
    .await
    {
        tracing::error!(source = %req.source, "build_image: docker build failed: {e}");
        send(event_error(format!("docker build failed: {e}"))).await;
        return;
    }
    tracing::info!(tag = %build_tag, "build_image: docker build complete");

    // ── export ────────────────────────────────────────────────────────────────
    tracing::info!(tag = %build_tag, "build_image: exporting container filesystem");
    send(event(Stage::Exporting, format!("exporting {}", req.source))).await;

    let rootfs = tmpdir.path().join("rootfs");
    if let Err(e) = tokio::fs::create_dir_all(&rootfs).await {
        send(event_error(format!("mkdir rootfs: {e}"))).await;
        return;
    }

    let container_id = match tokio::process::Command::new(bin)
        .args(["create", &build_tag])
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
            let _ = run_cmd(bin, &["rmi", &build_tag]).await;
            send(event_error(msg)).await;
            return;
        }
        Err(e) => {
            tracing::error!("build_image: container create: {e}");
            let _ = run_cmd(bin, &["rmi", &build_tag]).await;
            send(event_error(format!("container create: {e}"))).await;
            return;
        }
    };

    let mut docker_child = match tokio::process::Command::new(bin)
        .args(["export", &container_id])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = run_cmd(bin, &["rm", &container_id]).await;
            let _ = run_cmd(bin, &["rmi", &build_tag]).await;
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
            let _ = run_cmd(bin, &["rmi", &build_tag]).await;
            send(event_error(format!("spawn tar: {e}"))).await;
            return;
        }
    };

    let mut docker_stdout = docker_child.stdout.take().unwrap();
    let mut tar_stdin = tar_child.stdin.take().unwrap();
    if let Err(e) = tokio::io::copy(&mut docker_stdout, &mut tar_stdin).await {
        let _ = docker_child.kill().await;
        let _ = tar_child.kill().await;
        let _ = run_cmd(bin, &["rm", &container_id]).await;
        let _ = run_cmd(bin, &["rmi", &build_tag]).await;
        send(event_error(format!("streaming export to tar: {e}"))).await;
        return;
    }
    drop(tar_stdin);

    let docker_out = docker_child.wait_with_output().await;
    let tar_out = tar_child.wait_with_output().await;

    let _ = run_cmd(bin, &["rm", &container_id]).await;
    let _ = run_cmd(bin, &["rmi", &build_tag]).await;

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

    // ── bake in overlay-init and resolv.conf ─────────────────────────────────
    tracing::info!("build_image: baking overlay-init into rootfs");
    send(event(Stage::Exporting, "baking overlay-init into rootfs")).await;

    for dir in &["overlay", "rom"] {
        if let Err(e) = tokio::fs::create_dir_all(rootfs.join(dir)).await {
            send(event_error(format!("mkdir {dir}: {e}"))).await;
            return;
        }
    }

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

    let _ = tokio::fs::write(
        rootfs.join("etc/resolv.conf"),
        "nameserver 8.8.8.8\nnameserver 1.1.1.1\n",
    )
    .await;

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
