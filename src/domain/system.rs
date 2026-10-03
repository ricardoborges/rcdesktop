use serde::{Deserialize, Serialize};

/// Output of `wslc info`: client component versions and active sessions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WslcSystemInfo {
    pub version: String,
    pub kernel_version: String,
    pub windows_version: String,
    pub direct3d_version: String,
    pub dxcore_version: String,
    pub settings_file: String,
    pub session_manager_version: String,
    pub sessions: Vec<WslcSession>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WslcSession {
    pub id: String,
    pub name: String,
    pub creator_pid: String,
}

/// Cleanup operations offered on the WSLC page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneTarget {
    /// All stopped containers
    Containers,
    /// Dangling (untagged) images
    DanglingImages,
    /// Every image not used by a container
    UnusedImages,
    /// Networks not used by a container
    Networks,
    /// Volumes not referenced by a container, named ones included
    Volumes,
}

impl PruneTarget {
    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "containers" => Self::Containers,
            "images" => Self::DanglingImages,
            "images-all" => Self::UnusedImages,
            "networks" => Self::Networks,
            "volumes" => Self::Volumes,
            _ => return None,
        })
    }

    /// `wslc` arguments; `-f` skips the interactive confirmation.
    pub fn args(self) -> Vec<String> {
        let args: &[&str] = match self {
            Self::Containers => &["container", "prune", "-f"],
            Self::DanglingImages => &["image", "prune", "-f"],
            Self::UnusedImages => &["image", "prune", "-a", "-f"],
            Self::Networks => &["network", "prune", "-f"],
            Self::Volumes => &["volume", "prune", "-a", "-f"],
        };
        args.iter().map(|a| a.to_string()).collect()
    }
}
