use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What `goddard-agent scratch` asks of the daemon. Scratch directories
/// are daemon-owned storage keyed by the calling task's root id: they
/// survive machine restarts and are deleted when the owning task is
/// archived or removed. Contents are reproducible by contract — an agent
/// declares that by keeping work here rather than in the repository.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ScratchOperation {
    /// Ensure a scratch directory exists and return its absolute path.
    /// `name` selects a slug subdirectory of the task's scratch pool;
    /// absent selects the pool root itself.
    Get {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// List the task's existing named scratch directories.
    List,
}

/// One named scratch directory under a task's scratch pool.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScratchEntry {
    pub name: String,
    #[ts(type = "string")]
    pub path: PathBuf,
}
