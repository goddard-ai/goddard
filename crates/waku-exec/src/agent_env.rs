//! Configuration carried by a scoped agent launch.

use std::path::PathBuf;
use uuid::Uuid;

/// The agent surface one provider launch receives: the scoped credential,
/// the session it belongs to, the daemon's address, and where the
/// `goddard-agent` CLI lives. `DriverStartOptions` carries it to whichever
/// spawn path the provider uses.
#[derive(Clone, Debug)]
pub struct AgentLaunchEnv {
    pub token: String,
    /// The Waku task this runtime serves; the daemon attributes any prompt
    /// the token sends to it.
    pub task_id: Uuid,
    /// The task this session is a side chat of, when it is one. Delivered as
    /// `GODDARD_PARENT_TASK_ID` so the agent can find its parent's
    /// transcript without parsing it out of a prompt.
    pub parent_task_id: Option<Uuid>,
    /// The daemon's WebSocket address, as the daemon bound it.
    pub daemon_address: String,
    /// The `goddard-agent` executable.
    pub cli_path: PathBuf,
    /// A daemon-private directory for the session's CLI launcher. Providers
    /// whose sessions share one host process cannot receive per-session
    /// environment, so they write a credential-carrying shim here and point
    /// the session at it instead.
    pub shim_directory: PathBuf,
    /// Whether `goddard-agent create`/`prompt` — the opt-in cross-task surface —
    /// will accept calls on this credential.
    pub task_tools: bool,
    /// Whether the `goddard-agent command` settings writes will accept calls on
    /// this credential. On by default; off only when the user disabled the
    /// agent settings surface outright.
    pub settings_writes: bool,
    /// Whether this credential belongs to the Boss — its `search` scans
    /// every project's tasks rather than only its own project's.
    pub boss: bool,
    /// Whether `goddard-agent memory` will answer on this credential — the
    /// session is not a Boss principal and its task has a real project to
    /// scope a shared bucket to.
    pub memory: bool,
    /// The resource reservation the daemon already holds for this session —
    /// a summon admission ticket's granted id, exported as
    /// `GODDARD_RESOURCE_RESERVATION` so the agent's `resource` calls
    /// attach to the session's set instead of re-queueing.
    pub resource_reservation: Option<Uuid>,
}

impl AgentLaunchEnv {
    /// The scopes this launch carries, for composing the session's
    /// agent-surface instruction through whichever channel delivers it.
    pub fn scope(&self) -> AgentSurfaceScope {
        AgentSurfaceScope {
            task_tools: self.task_tools,
            settings_writes: self.settings_writes,
            parent_task_id: self.parent_task_id,
            boss: self.boss,
            memory: self.memory,
        }
    }
}

/// Which `goddard-agent` subcommands a session's credential will accept —
/// everything the agent-surface instruction needs that isn't the command
/// name itself.
#[derive(Clone, Copy, Debug)]
pub struct AgentSurfaceScope {
    pub task_tools: bool,
    pub settings_writes: bool,
    pub parent_task_id: Option<Uuid>,
    /// Boss credentials search every project the daemon knows; ordinary
    /// credentials search only their own project.
    pub boss: bool,
    /// The session's project memory surface: `goddard-agent memory` against
    /// the session project's shared bucket.
    pub memory: bool,
}
