//! Rust transport and lifecycle support for clients of `goddard-daemon`.
//!
//! This crate intentionally depends on [`waku_protocol`] and [`waku_share`]
//! (for LAN discovery) only, so GUI and CLI clients cannot accidentally
//! reach daemon-owned filesystem, Git, database, or provider
//! implementations.

mod client;
pub mod command_env;
pub mod composer_complete;
pub mod computer_use;
mod daemons;
pub mod discover;
pub mod driver;
pub mod persistence;
mod process;
mod workspace_client;

pub use client::{DaemonClient, PairReply, pair};
pub use discover::DaemonDiscovery;
pub use daemons::{DaemonKey, DaemonMap};
pub use process::{
    DEFAULT_EXPOSED_DAEMON_PORT, DaemonExposureSettings, DaemonProcess, DaemonStatus,
    DaemonSupervisor, parse_allowed_origins,
};
pub use waku_protocol::*;
pub use workspace_client::WorkspaceClient;

pub mod git_branch {
    pub use waku_protocol::git::{BranchEntry, BranchSnapshot};
}

pub mod git_commit {
    pub use waku_protocol::git::AgentInvocation;
    pub use waku_protocol::git::ArchivePreview;
    pub use waku_protocol::git::CheckoutStatus;
    pub use waku_protocol::git::CommitSnapshot as Snapshot;
    pub use waku_protocol::git::StatusEntry;
}

pub mod worktree {
    pub use waku_protocol::git::CreatedWorktree;
}
