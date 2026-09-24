#![recursion_limit = "256"]

//! Goddard's shared, versioned wire contract.
//!
//! This crate contains serializable data only. It performs no provider,
//! database, workspace, Git, attachment, or transport I/O, so native and web
//! clients can depend on it without pulling in the daemon implementation.
//! The one exception is the `.waku` → `.goddard` adoption layer — `migration`
//! and `pid` touch the filesystem and host processes because the migration
//! conventions (artifact names, link policy) must be shared by every build
//! that can launch, and no daemon owns the app's home directory.

rust_i18n::i18n!("../../locales", fallback = "en");

// `i18n!` reads these files in a proc macro. Explicit includes make them
// visible to Cargo's dependency tracker, so locale-only edits rebuild this
// shared translation registry under the development watcher.
const _LOCALE_SOURCES: [&str; 3] = [
    include_str!("../../../locales/app.yml"),
    include_str!("../../../locales/zh-CN.yml"),
    include_str!("../../../locales/ja.yml"),
];

macro_rules! tr {
    ($key:expr) => {
        crate::i18n::translate($key)
    };
    ($key:expr, $($args:tt)*) => {
        rust_i18n::t!($key, $($args)*).into_owned()
    };
}

pub mod attachments;
pub mod auto_prompts;
pub mod automations;
pub mod blob;
pub mod checkpoint;
pub mod composer;
pub mod computer_use;
pub mod custom_commands;
mod driver_wire;
pub mod eval;
pub mod exposure;
pub mod friends;
pub mod git;
pub mod i18n;
pub mod identity;
pub mod integrations;
pub mod migration;
pub mod model;
pub mod model_catalog;
pub mod pairing;
pub mod persistence;
pub mod pid;
pub mod projectless;
pub mod provider_session;
pub mod routing;
pub mod settings;
pub mod skills;
pub mod theme;
pub mod usage;
pub mod usage_history;
pub mod workspace;

mod protocol;

pub use driver_wire::{decode_enum, encode_enum, event_from_wire, event_to_wire};
pub use exposure::{DaemonExposure, parse_allowed_origins};
pub use protocol::{
    AGENT_ASK_REQUEST_PREFIX, AGENT_PARENT_TASK_ENV, AGENT_RENAME_REQUEST_PREFIX, AGENT_TASK_ENV,
    AGENT_TOKEN_ENV, APP_EXECUTABLE_ENV, AgentPromptDelivery, AgentWorkspace, ClientMessage,
    Command, DAEMON_ADDRESS_ENV, DAEMON_TOKEN_ENV, DaemonChildKind, DaemonChildSample, DaemonReady,
    DaemonSessionSample, DaemonStatsSample, MAX_WIRE_MESSAGE_BYTES, PROTOCOL_VERSION, ReplayCursor,
    Request, ResponseOutcome, ResponsePayload, RpcError, SequencedEvent, ServerMessage,
    SessionDetailTail, TASK_LINK_PREFIX, WireComputerToolRequest, WireDriverEvent,
    WireDriverStartOptions, WireSessionOptions,
};
pub use protocol::{KeyedError, WireTranslation};
pub use settings::DaemonSettings;
pub use workspace::{
    BranchDeleteFailure, GitHubAvailability, GitHubRelease, GitHubReleaseAsset, GitHubRepoRef,
    GitHubWorkflowJob, GitHubWorkflowRun, GitHubWorkflowStep, IssueDetail, IssueState,
    IssueSummary, NotificationPoll, NotificationReason, NotificationSubjectType,
    NotificationThread, PullRequestCheck, PullRequestCheckStatus, PullRequestCommit,
    PullRequestDetail, PullRequestFile, PullRequestReviewComment, PullRequestReviewDecision,
    PullRequestState, PullRequestSummary, RepoBranch, RepoWorktree, WorkItemComment, WorkItemKind,
    WorkItemQueryState, WorkspaceOperation, WorkspaceResult,
};
