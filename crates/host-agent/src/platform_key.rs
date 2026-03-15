use russh::client;
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey, load_secret_key};

pub struct SshClientHandler;

impl client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true) // trust our own VMs; host key verified by network isolation
    }
}

pub fn platform_key_path() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("PLATFORM_KEY_PATH").unwrap_or_else(|_| "/var/lib/spwn/platform_key".into()),
    )
}

pub fn load_or_generate_platform_key() -> anyhow::Result<PrivateKey> {
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

/// Inject the platform SSH public key into `rootfs/root/.ssh/authorized_keys`.
/// Returns `Ok(true)` if the key was present and injected, `Ok(false)` if the
/// platform key file doesn't exist yet (agent hasn't generated it).
pub async fn inject_platform_pubkey(rootfs: &std::path::Path) -> anyhow::Result<bool> {
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
