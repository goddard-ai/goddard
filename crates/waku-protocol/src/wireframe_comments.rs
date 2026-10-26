//! Durable, task-scoped discussion attached to stable wireframe elements.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Typed operations shared by desktop clients and scoped agents.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum WireframeCommentOperation {
    Get {
        thread: Uuid,
    },
    List {
        #[serde(default)]
        offset: u32,
    },
    Add {
        screen: String,
        anchor: Option<String>,
        body: String,
        key: String,
    },
    Reply {
        thread: Uuid,
        body: String,
        key: String,
    },
    Read {
        thread: Uuid,
        message: Uuid,
    },
    Resolve {
        thread: Uuid,
        resolved: bool,
    },
}

/// Append-only message. Identity is assigned by the daemon.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WireframeCommentMessage {
    pub id: Uuid,
    pub author_id: String,
    pub author_label: String,
    pub body: String,
    pub created_at: u64,
}

/// Reading changes `unread`, never `resolved` or the existence of the thread.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WireframeCommentThread {
    pub id: Uuid,
    pub screen: String,
    pub anchor: Option<String>,
    pub label: String,
    pub revision: String,
    pub resolved: bool,
    pub messages: Vec<WireframeCommentMessage>,
    pub unread: bool,
}

/// A bounded page from one document; only the active document is loaded.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WireframeCommentsPage {
    pub document: String,
    pub path: String,
    pub revision: String,
    pub threads: Vec<WireframeCommentThread>,
    pub next_offset: Option<u32>,
}
