#![recursion_limit = "256"]

//! Provider drivers, discovery, integrations, and cloud runtimes for Goddard.
//! `waku-core` re-exports these modules to preserve existing paths.

#[macro_use]
extern crate waku_base;

pub(crate) use waku_base::{blob_store, fs_ext, http_wire, i18n, identity, settings};
pub(crate) use waku_exec::{command_env, sandbox};
pub(crate) use waku_git::git_branch;
pub(crate) use waku_sessions::{
    acp_session, agent, amp_session, claude_metadata, claude_session, codex_session, computer_use,
    copilot_session, cursor_session, deepseek_pool, deepseek_session, devin_session, eval,
    grok_session, kimi_session, muse_service, muse_session, opencode_api, opencode_service,
    opencode_session, subagents, usage,
};

pub mod cloud;
pub mod driver;
pub mod integrations;
pub mod model;
pub mod model_catalog;
pub mod permission_review;
pub mod slash_command_catalog;
