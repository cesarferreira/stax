pub mod history;

use serde::{Deserialize, Serialize};

/// Provider-neutral CI check details used by the forge layer and CI command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckRunInfo {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_percent: Option<u8>,
}
