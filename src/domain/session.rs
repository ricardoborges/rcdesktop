use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WslcSessionInfo {
    pub session_id: String,
    pub is_elevated: bool,
    pub is_healthy: bool,
    pub status_message: String,
    pub version: String,
}

impl Default for WslcSessionInfo {
    fn default() -> Self {
        Self {
            session_id: "unknown".into(),
            is_elevated: false,
            is_healthy: false,
            status_message: "Initializing...".into(),
            version: "unknown".into(),
        }
    }
}
