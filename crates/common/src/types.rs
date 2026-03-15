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
    pub vcpus: i64,
    pub memory_mb: u32,
    pub kernel_path: PathBuf,
    pub rootfs_path: PathBuf,
    pub exposed_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmResources {
    pub vcpus: i64,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_id_new_and_as_str() {
        let id = VmId::new("abc-123");
        assert_eq!(id.as_str(), "abc-123");
        assert_eq!(id.0, "abc-123");
    }

    #[test]
    fn test_vm_id_display() {
        let id = VmId::new("my-vm");
        assert_eq!(id.to_string(), "my-vm");
    }

    #[test]
    fn test_vm_id_equality() {
        assert_eq!(VmId::new("x"), VmId::new("x"));
        assert_ne!(VmId::new("x"), VmId::new("y"));
    }

    #[test]
    fn test_vm_status_display() {
        assert_eq!(VmStatus::Stopped.to_string(), "stopped");
        assert_eq!(VmStatus::Starting.to_string(), "starting");
        assert_eq!(VmStatus::Running.to_string(), "running");
        assert_eq!(VmStatus::Snapshotting.to_string(), "snapshotting");
        assert_eq!(VmStatus::Paused.to_string(), "paused");
        assert_eq!(VmStatus::Error.to_string(), "error");
    }
}
