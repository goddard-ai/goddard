//! Command execution, sandbox processes, and daemon-owned terminals.
//! `waku-core` re-exports these modules to preserve existing paths.

#[macro_use]
extern crate waku_base;

mod agent_env;
pub use agent_env::{AgentLaunchEnv, AgentSurfaceScope};
pub(crate) use waku_base::subprocess;

pub mod runtime_resources;
pub use runtime_resources::agent_cli_path;

pub mod command_env;
pub mod sandbox;
pub mod terminal;
