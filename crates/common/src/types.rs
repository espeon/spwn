use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VmId(pub String);

impl VmId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for VmId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    pub id: VmId,
    pub vcores: u8,
    pub memory_mb: u32,
    pub kernel_path: PathBuf,
    pub rootfs_path: PathBuf,
    pub exposed_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmResources {
    pub vcores: u8,
    pub memory_mb: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VmStatus {
    Stopped,
    Starting,
    Running,
    Snapshotting,
    Paused,
    Error,
}

impl std::fmt::Display for VmStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            VmStatus::Stopped => "stopped",
            VmStatus::Starting => "starting",
            VmStatus::Running => "running",
            VmStatus::Snapshotting => "snapshotting",
            VmStatus::Paused => "paused",
            VmStatus::Error => "error",
        };
        write!(f, "{s}")
    }
}
