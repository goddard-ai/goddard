#![recursion_limit = "256"]

//! Goddard's daemon-side core.
//!
//! Provider, database, filesystem, and Git implementations live here, behind
//! the transport-neutral contract in `waku-protocol`. Client applications
//! intentionally depend on `waku-client` instead of this crate.

// `tr!`, `keyed!`, and `localized!` live in `waku-base` now; importing them at
// the crate root keeps every module's unqualified calls working.
#[macro_use]
extern crate waku_base;

pub use waku_base::{keyed, localized, tr};

pub mod acp_session;
pub mod agent;
mod agent_merge;
pub mod amp_session;
pub mod auto_prompts;
pub mod automations;
pub mod boss;
pub mod boss_context;
pub mod boss_eval;
pub mod boss_rotation;
pub use waku_git::checkpoint;
mod claude_metadata;
pub mod claude_session;
pub mod cloud;
pub mod codex_session;
pub use waku_exec::command_env;
pub mod composer_complete;
pub mod computer_use;
pub mod copilot_session;
pub mod cursor_session;
pub mod daemon;
pub mod deepseek_pool;
pub mod deepseek_session;
pub mod devin_session;
pub mod driver;
pub mod eval;
pub use waku_git::git_branch;
pub use waku_git::git_commit;
pub use waku_git::git_panel;
pub mod github;
pub mod grok_session;
pub mod inference;
pub mod integrations;
pub mod issues;
pub mod kimi_session;
pub mod migration;
pub mod model;
pub mod model_catalog;
pub mod muse_service;
pub mod muse_session;
pub mod notifications;
pub mod opencode_api;
pub mod opencode_service;
pub mod opencode_session;
pub mod permission_review;
pub mod persistence;
pub mod pi_session;
pub mod pull_requests;
pub mod repo;
pub use waku_repo_map as repo_map;
pub mod review;
pub mod routing;
pub use waku_exec::sandbox;
pub mod share;
pub mod shell_command;
pub mod skills;
mod slash_command_catalog;
pub mod stats;
mod subagents;
pub mod sync;
pub use waku_exec::terminal;
pub mod usage;
pub mod usage_history;
pub mod whistle;
pub mod workspace;
pub use waku_git::worktree;

mod server;

// `waku-base` leaves, re-exported so `waku_core::x` paths keep working while
// the crate split lands in stages.
pub use waku_base::attachments;
pub use waku_base::blob_store;
pub(crate) use waku_base::frontmatter;
pub(crate) use waku_base::fs_ext;
pub(crate) use waku_base::http_wire;
pub use waku_base::i18n;
pub use waku_base::identity;
pub use waku_base::issue_templates;
pub use waku_base::lan;
pub use waku_base::pairing;
pub use waku_base::power;
pub(crate) use waku_base::pressure;
pub use waku_base::projectless;
pub(crate) use waku_base::protocol;
pub use waku_base::resource_broker;
pub use waku_base::settings;
pub(crate) use waku_base::subprocess;
pub use waku_base::theme;

pub use protocol::{
    AGENT_TASK_ENV, AGENT_TOKEN_ENV, APP_EXECUTABLE_ENV, AgentPromptDelivery, AgentWorkspace,
    ClientMessage, Command, DAEMON_ADDRESS_ENV, DAEMON_TOKEN_ENV, DaemonReady, PROTOCOL_VERSION,
    ReplayCursor, Request, ResponseOutcome, ResponsePayload, RpcError, SequencedEvent,
    ServerMessage, SessionDetailTail, WireComputerToolRequest, WireDriverEvent,
    WireDriverStartOptions, WireSessionOptions,
};
pub use server::{Backend, EventSink, ServerOptions, serve};
pub use settings::{DaemonSettings, DaemonSettingsStore};
pub use workspace::{WorkspaceOperation, WorkspaceResult};
