//! Provider-neutral projects, sessions, transcript items, and driver events.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::git::CommitEntry;
use crate::routing::RouteDecision;

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, TS,
)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    Antigravity,
    Amp,
    Claude,
    #[default]
    Codex,
    Copilot,
    Cursor,
    DeepSeek,
    Devin,
    Droid,
    Fx,
    OpenCode,
    OpenCode2,
    Goose,
    Grok,
    Kimi,
    Muse,
    OhMyPi,
    Pi,
}

impl ProviderKind {
    pub const ALL: [Self; 18] = [
        Self::Antigravity,
        Self::Amp,
        Self::Claude,
        Self::Codex,
        Self::Copilot,
        Self::Cursor,
        Self::DeepSeek,
        Self::Devin,
        Self::Droid,
        Self::Fx,
        Self::OpenCode,
        Self::OpenCode2,
        Self::Goose,
        Self::Grok,
        Self::Kimi,
        Self::Muse,
        Self::OhMyPi,
        Self::Pi,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Antigravity => "antigravity",
            Self::Amp => "amp",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
            Self::Cursor => "cursor",
            Self::DeepSeek => "deepseek",
            Self::Devin => "devin",
            Self::Droid => "droid",
            Self::Fx => "fx",
            Self::OpenCode => "opencode",
            Self::OpenCode2 => "opencode2",
            Self::Goose => "goose",
            Self::Grok => "grok",
            Self::Kimi => "kimi",
            Self::Muse => "muse",
            Self::OhMyPi => "ohmypi",
            Self::Pi => "pi",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Antigravity => "Antigravity CLI",
            Self::Amp => "Amp",
            Self::Claude => "Claude Code",
            Self::Codex => "Codex CLI",
            Self::Copilot => "GitHub Copilot",
            Self::Cursor => "Cursor CLI",
            Self::DeepSeek => "DeepSeek Harness",
            Self::Devin => "Devin CLI",
            Self::Droid => "Droid",
            Self::Fx => "Fx",
            Self::OpenCode => "OpenCode",
            Self::OpenCode2 => "OpenCode 2",
            Self::Goose => "Goose",
            Self::Grok => "Grok Build",
            Self::Kimi => "Kimi Code",
            Self::Muse => "Muse Code",
            Self::OhMyPi => "Oh My Pi",
            Self::Pi => "Pi",
        }
    }

    /// Whether this provider's CLI can run inside the sandbox VM. The
    /// daemon enforces it and clients explain it, so the answer lives on
    /// the wire type both sides share.
    ///
    /// Providers are excluded when their transport cannot ride the guest's
    /// stdio channel: DeepSeek and Muse multiplex every session on one
    /// resident host process, OpenCode and OpenCode 2 speak loopback HTTP
    /// to a server the daemon must reach over TCP, Copilot's SDK owns its
    /// process spawn, and Antigravity is its own terminal TUI with no
    /// daemon driver at all.
    pub fn supports_sandbox(self) -> bool {
        matches!(
            self,
            Self::Codex
                | Self::Claude
                | Self::Amp
                | Self::Cursor
                | Self::Devin
                | Self::Droid
                | Self::Fx
                | Self::Goose
                | Self::Grok
                | Self::Kimi
                | Self::OhMyPi
                | Self::Pi
        )
    }

    /// Whether the provider offers a hosted cloud environment a task can be
    /// submitted to — the third Environment choice beside This Mac and the
    /// Sandbox VM. Same shared-type reasoning as [`Self::supports_sandbox`].
    pub fn supports_cloud(self) -> bool {
        matches!(
            self,
            Self::Claude | Self::Codex | Self::Copilot | Self::Cursor | Self::Devin | Self::Droid
        )
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Self::Antigravity => "Antigravity",
            Self::Amp => "Amp",
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Copilot => "Copilot",
            Self::Cursor => "Cursor",
            Self::DeepSeek => "DeepSeek",
            Self::Devin => "Devin",
            Self::Droid => "Droid",
            Self::Fx => "Fx",
            Self::OpenCode => "OpenCode",
            Self::OpenCode2 => "OpenCode 2",
            Self::Goose => "Goose",
            Self::Grok => "Grok",
            Self::Kimi => "Kimi",
            Self::Muse => "Muse",
            Self::OhMyPi => "Oh My Pi",
            Self::Pi => "Pi",
        }
    }

    pub fn command(self) -> &'static str {
        match self {
            Self::Antigravity => "agy",
            Self::Amp => "amp",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
            // Cursor documents `agent` as its primary command, but that name is
            // shared by other CLIs. The backward-compatible alias is unambiguous.
            Self::Cursor => "cursor-agent",
            Self::DeepSeek => "dsh",
            Self::Devin => "devin",
            Self::Droid => "droid",
            Self::Fx => "fx",
            Self::OpenCode => "opencode",
            Self::OpenCode2 => "opencode2",
            Self::Goose => "goose",
            Self::Grok => "grok",
            Self::Kimi => "kimi",
            Self::Muse => "muse",
            Self::OhMyPi => "omp",
            Self::Pi => "pi",
        }
    }

    /// Whether Resume can enumerate this provider's native sessions at all.
    /// Antigravity conversations live in its own TUI with no readable store,
    /// so it is excluded statically rather than discovered by probing.
    pub fn supports_session_catalog(self) -> bool {
        !matches!(self, Self::Antigravity)
    }

    /// Vendor-documented setup for the provider's CLI: the canonical one-line
    /// install command, the interactive sign-in command, and the docs page.
    /// The Settings page shows these verbatim and can run them in a terminal.
    pub fn setup(self) -> ProviderSetup {
        match self {
            // Antigravity signs in inside its own TUI, so the sign-in step
            // simply launches `agy`.
            Self::Antigravity => ProviderSetup {
                install: "curl -fsSL https://antigravity.google/cli/install.sh | bash",
                update: Some("agy update"),
                sign_in: Some("agy"),
                api_key_env: Some("GEMINI_API_KEY"),
                docs_url: "https://antigravity.google/docs/cli/overview",
            },
            Self::Amp => ProviderSetup {
                install: "curl -fsSL https://ampcode.com/install.sh | bash",
                update: Some("amp update"),
                sign_in: Some("amp login"),
                api_key_env: Some("AMP_API_KEY"),
                docs_url: "https://ampcode.com/manual",
            },
            Self::Claude => ProviderSetup {
                install: "curl -fsSL https://claude.ai/install.sh | bash",
                update: Some("claude update"),
                sign_in: Some("claude auth login"),
                api_key_env: Some("ANTHROPIC_API_KEY"),
                docs_url: "https://code.claude.com/docs/en/install",
            },
            // Codex reads an API key via `codex login --with-api-key`, not
            // silently from the environment.
            Self::Codex => ProviderSetup {
                install: "curl -fsSL https://chatgpt.com/codex/install.sh | sh",
                update: Some("codex update"),
                sign_in: Some("codex login"),
                api_key_env: None,
                docs_url: "https://developers.openai.com/codex/cli",
            },
            // Copilot signs in inside its own TUI (`/login`), and headless
            // sessions take a token from the environment — the SDK honors
            // COPILOT_GITHUB_TOKEN, GH_TOKEN, and GITHUB_TOKEN.
            Self::Copilot => ProviderSetup {
                install: "npm install -g @github/copilot",
                update: Some("copilot update"),
                sign_in: Some("copilot"),
                api_key_env: Some("COPILOT_GITHUB_TOKEN"),
                docs_url: "https://docs.github.com/en/copilot/how-tos/set-up/install-copilot-cli",
            },
            Self::Cursor => ProviderSetup {
                install: "curl -fsSL https://cursor.com/install | bash",
                update: Some("cursor-agent update"),
                sign_in: Some("cursor-agent login"),
                api_key_env: Some("CURSOR_API_KEY"),
                docs_url: "https://cursor.com/docs/cli/installation",
            },
            // DeepSeek Harness authenticates with an API key only; there is
            // no login subcommand.
            Self::DeepSeek => ProviderSetup {
                install: "npm install -g @deepseek-ai/dsh",
                update: None,
                sign_in: None,
                api_key_env: Some("DEEPSEEK_API_KEY"),
                docs_url: "https://github.com/deepseek-ai/deepseek-harness",
            },
            Self::Devin => ProviderSetup {
                install: "curl -fsSL https://cli.devin.ai/install.sh | bash",
                update: Some("devin update"),
                sign_in: Some("devin auth login"),
                api_key_env: None,
                docs_url: "https://docs.devin.ai/cli",
            },
            // Droid signs in through the browser from inside its TUI; the
            // API key is for headless `droid exec` runs.
            Self::Droid => ProviderSetup {
                install: "curl -fsSL https://app.factory.ai/cli | sh",
                update: None,
                sign_in: Some("droid"),
                api_key_env: Some("FACTORY_API_KEY"),
                docs_url: "https://docs.factory.ai/droid-cli/quickstart",
            },
            Self::Fx => ProviderSetup {
                install: "curl -fsSL https://fx.sh/setup.sh | bash",
                update: Some("fx upgrade"),
                sign_in: Some("fx login"),
                api_key_env: Some("AI_GATEWAY_API_KEY"),
                docs_url: "https://fx.sh/docs/getting-started/installation",
            },
            Self::OpenCode => ProviderSetup {
                install: "curl -fsSL https://opencode.ai/install | bash",
                update: Some("opencode upgrade"),
                sign_in: Some("opencode auth login"),
                api_key_env: None,
                docs_url: "https://opencode.ai/docs",
            },
            Self::OpenCode2 => ProviderSetup {
                install: "curl -fsSL https://opencode.ai/v2/install | bash",
                update: Some("opencode2 upgrade"),
                sign_in: Some("opencode2 auth login"),
                api_key_env: None,
                docs_url: "https://opencode.ai/v2/docs",
            },
            // Goose is multi-provider; `goose configure` interactively picks
            // the provider and model and stores credentials in its own config.
            Self::Goose => ProviderSetup {
                install: "curl -fsSL https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh | bash",
                update: Some("goose update"),
                sign_in: Some("goose configure"),
                api_key_env: None,
                docs_url: "https://aaif-goose.github.io/goose/",
            },
            Self::Grok => ProviderSetup {
                install: "curl -fsSL https://x.ai/cli/install.sh | bash",
                update: None,
                sign_in: Some("grok login"),
                api_key_env: Some("XAI_API_KEY"),
                docs_url: "https://docs.x.ai/build/overview",
            },
            Self::Kimi => ProviderSetup {
                install: "curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash",
                update: None,
                sign_in: Some("kimi login"),
                api_key_env: None,
                docs_url: "https://www.kimi.com/code/docs/en/kimi-code-cli/guides/getting-started.html",
            },
            // Muse credentials live in the Muse CLI itself; MSP exposes no
            // login surface, so sign-in is always the CLI's own flow.
            Self::Muse => ProviderSetup {
                install: "curl -fsSL https://dev.meta.ai/install.sh | sh",
                update: None,
                sign_in: Some("muse login"),
                api_key_env: None,
                docs_url: "https://meta-models.github.io/muse-code-sdk",
            },
            Self::OhMyPi => ProviderSetup {
                install: "curl -fsSL https://omp.sh/install | sh",
                update: None,
                sign_in: Some("omp auth-broker login"),
                api_key_env: None,
                docs_url: "https://github.com/can1357/oh-my-pi",
            },
            // Pi has no login subcommand; signing in is `/login` inside its
            // TUI, so the sign-in step simply launches `pi`.
            Self::Pi => ProviderSetup {
                install: "curl -fsSL https://pi.dev/install.sh | sh",
                update: Some("pi update"),
                sign_in: Some("pi"),
                api_key_env: None,
                docs_url: "https://pi.dev/docs/latest",
            },
        }
    }

    /// Kimi Code, Fx, Devin, Droid, Goose, and Antigravity are deliberately
    /// absent from this list and from [`Self::supports_conversation_fork`].
    /// Kimi's ACP `session/fork` copies a whole session and takes no turn
    /// count, and Fx, Devin, Droid, and Goose expose no turn-aware fork or
    /// truncation method.
    /// Antigravity is terminal-backed: its sessions are its own TUI, not
    /// Goddard turns. None of them can reproduce Goddard's "drop the last N
    /// turns" semantics without corrupting history.
    /// Copilot is present: `sessions.fork` truncates at an event boundary, and
    /// rewinding resumes the task on the truncated fork.
    pub fn supports_conversation_rollback(self) -> bool {
        matches!(
            self,
            Self::Amp
                | Self::Claude
                | Self::Codex
                | Self::Copilot
                | Self::Cursor
                | Self::DeepSeek
                | Self::OpenCode
                | Self::OpenCode2
                | Self::Grok
                | Self::Muse
                | Self::OhMyPi
                | Self::Pi
        )
    }

    pub fn supports_conversation_fork(self) -> bool {
        matches!(
            self,
            Self::Amp
                | Self::Claude
                | Self::Codex
                | Self::Copilot
                | Self::Cursor
                | Self::DeepSeek
                | Self::OpenCode
                | Self::OpenCode2
                | Self::Grok
                | Self::Muse
                | Self::OhMyPi
                | Self::Pi
        )
    }

    pub fn supports_model_discovery(self) -> bool {
        matches!(
            self,
            Self::Antigravity
                | Self::Claude
                | Self::Codex
                | Self::Copilot
                | Self::Cursor
                | Self::DeepSeek
                | Self::Devin
                | Self::Droid
                | Self::Fx
                | Self::OpenCode
                | Self::OpenCode2
                | Self::Grok
                | Self::Kimi
                | Self::Muse
                | Self::OhMyPi
                | Self::Pi
        )
    }

    /// Transports with a dedicated compact RPC the daemon calls directly —
    /// Codex's `thread/compact/start` and OpenCode 2's
    /// `POST /api/session/{id}/compact`. For every other provider a compact
    /// path exists only when the provider's own command catalog reports one,
    /// so this predicate marks the transports whose composer `/compact` entry
    /// is a Waku-reserved builtin that shadows any provider-reported command.
    pub fn supports_compact(self) -> bool {
        matches!(self, Self::Codex | Self::OpenCode2)
    }
}

/// Vendor-documented setup for a provider's CLI — see
/// [`ProviderKind::setup`].
#[derive(Clone, Copy, Debug)]
pub struct ProviderSetup {
    /// The provider's canonical one-line install command.
    pub install: &'static str,
    /// The CLI's own update command when it has one. `None` means re-running
    /// `install` is the update path — every documented installer fetches the
    /// latest release.
    pub update: Option<&'static str>,
    /// Interactive sign-in command, `None` when the provider authenticates
    /// with an API key rather than a login flow.
    pub sign_in: Option<&'static str>,
    /// Environment variable that carries credentials directly, when the
    /// provider documents one.
    pub api_key_env: Option<&'static str>,
    /// Install/authentication documentation.
    pub docs_url: &'static str,
}

impl ProviderSetup {
    /// The command that brings an installed CLI to the latest release.
    pub fn update_command(&self) -> &'static str {
        self.update.unwrap_or(self.install)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "provider"
)]
pub enum ProviderResumeCursor {
    Antigravity {
        conversation_id: String,
    },
    Amp {
        thread_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fork_context: Option<String>,
    },
    Claude {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resume_at: Option<String>,
    },
    Codex {
        thread_id: String,
    },
    Copilot {
        session_id: String,
    },
    Cursor {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fork_context: Option<String>,
    },
    OpenCode {
        session_id: String,
    },
    OpenCode2 {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        directory: Option<String>,
    },
    DeepSeek {
        session_id: String,
    },
    Devin {
        session_id: String,
    },
    Droid {
        session_id: String,
    },
    Fx {
        session_id: String,
    },
    Goose {
        session_id: String,
    },
    Grok {
        session_id: String,
    },
    Kimi {
        session_id: String,
    },
    Muse {
        session_id: String,
        /// The last view cursor this runtime observed; a reattach can ask
        /// `session/resume` for the suffix only instead of a full fold.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        view_cursor: Option<String>,
    },
    OhMyPi {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_file: Option<PathBuf>,
    },
    Pi {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_file: Option<PathBuf>,
    },
}

impl ProviderResumeCursor {
    pub fn from_session_id(provider: ProviderKind, id: String) -> Self {
        match provider {
            ProviderKind::Antigravity => Self::Antigravity {
                conversation_id: id,
            },
            ProviderKind::Amp => Self::Amp {
                thread_id: id,
                fork_context: None,
            },
            ProviderKind::Claude => Self::Claude {
                session_id: id,
                resume_at: None,
            },
            ProviderKind::Codex => Self::Codex { thread_id: id },
            ProviderKind::Copilot => Self::Copilot { session_id: id },
            ProviderKind::Cursor => Self::Cursor {
                session_id: id,
                fork_context: None,
            },
            ProviderKind::DeepSeek => Self::DeepSeek { session_id: id },
            ProviderKind::Devin => Self::Devin { session_id: id },
            ProviderKind::Droid => Self::Droid { session_id: id },
            ProviderKind::Fx => Self::Fx { session_id: id },
            ProviderKind::OpenCode => Self::OpenCode { session_id: id },
            ProviderKind::OpenCode2 => Self::OpenCode2 {
                session_id: id,
                directory: None,
            },
            ProviderKind::Goose => Self::Goose { session_id: id },
            ProviderKind::Grok => Self::Grok { session_id: id },
            ProviderKind::Kimi => Self::Kimi { session_id: id },
            ProviderKind::Muse => Self::Muse {
                session_id: id,
                view_cursor: None,
            },
            ProviderKind::OhMyPi => Self::OhMyPi {
                session_id: id,
                session_file: None,
            },
            ProviderKind::Pi => Self::Pi {
                session_id: id,
                session_file: None,
            },
        }
    }

    pub fn provider(&self) -> ProviderKind {
        match self {
            Self::Antigravity { .. } => ProviderKind::Antigravity,
            Self::Amp { .. } => ProviderKind::Amp,
            Self::Claude { .. } => ProviderKind::Claude,
            Self::Codex { .. } => ProviderKind::Codex,
            Self::Copilot { .. } => ProviderKind::Copilot,
            Self::Cursor { .. } => ProviderKind::Cursor,
            Self::DeepSeek { .. } => ProviderKind::DeepSeek,
            Self::Devin { .. } => ProviderKind::Devin,
            Self::Droid { .. } => ProviderKind::Droid,
            Self::Fx { .. } => ProviderKind::Fx,
            Self::OpenCode { .. } => ProviderKind::OpenCode,
            Self::OpenCode2 { .. } => ProviderKind::OpenCode2,
            Self::Goose { .. } => ProviderKind::Goose,
            Self::Grok { .. } => ProviderKind::Grok,
            Self::Kimi { .. } => ProviderKind::Kimi,
            Self::Muse { .. } => ProviderKind::Muse,
            Self::OhMyPi { .. } => ProviderKind::OhMyPi,
            Self::Pi { .. } => ProviderKind::Pi,
        }
    }

    pub fn native_id(&self) -> &str {
        match self {
            Self::Antigravity { conversation_id } => conversation_id,
            Self::Amp { thread_id, .. } => thread_id,
            Self::Claude { session_id, .. }
            | Self::Copilot { session_id }
            | Self::Cursor { session_id, .. }
            | Self::DeepSeek { session_id }
            | Self::Devin { session_id }
            | Self::Droid { session_id }
            | Self::Fx { session_id }
            | Self::OpenCode { session_id }
            | Self::OpenCode2 { session_id, .. }
            | Self::Goose { session_id }
            | Self::Grok { session_id }
            | Self::Kimi { session_id }
            | Self::Muse { session_id, .. }
            | Self::OhMyPi { session_id, .. }
            | Self::Pi { session_id, .. } => session_id,
            Self::Codex { thread_id } => thread_id,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeMode {
    /// Older state files used `plan` as a combined read-only mode. Keep those
    /// sessions readable without retaining it as a product mode.
    Ask,
    #[default]
    AutoAcceptEdits,
    Auto,
    FullAccess,
}

impl<'de> Deserialize<'de> for RuntimeMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match String::deserialize(deserializer)?.as_str() {
            "plan" | "ask" => Ok(Self::Ask),
            "autoAcceptEdits" => Ok(Self::AutoAcceptEdits),
            "auto" => Ok(Self::Auto),
            "fullAccess" => Ok(Self::FullAccess),
            other => Err(<D::Error as serde::de::Error>::unknown_variant(
                other,
                &["ask", "autoAcceptEdits", "auto", "fullAccess"],
            )),
        }
    }
}

impl RuntimeMode {
    pub const ACCESS_OPTIONS: [Self; 4] = [
        Self::Ask,
        Self::AutoAcceptEdits,
        Self::Auto,
        Self::FullAccess,
    ];

    pub fn label(self) -> String {
        match self {
            Self::Ask => tr!("mode.supervised"),
            Self::AutoAcceptEdits => tr!("mode.auto_accept_edits"),
            Self::Auto => tr!("mode.auto"),
            Self::FullAccess => tr!("mode.full_access"),
        }
    }

    pub fn description(self) -> String {
        match self {
            Self::Ask => tr!("mode.supervised_description"),
            Self::AutoAcceptEdits => tr!("mode.auto_accept_edits_description"),
            Self::Auto => tr!("mode.auto_description"),
            Self::FullAccess => tr!("mode.full_access_description"),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Ask => "icons/lock.svg",
            Self::AutoAcceptEdits => "icons/pencil.svg",
            Self::Auto => "icons/sparkle.svg",
            Self::FullAccess => "icons/lock-open.svg",
        }
    }
}

/// Where a task's work runs: the host, the local sandbox VM, or the
/// provider's hosted cloud. Picked on the draft and fixed once the session
/// boots — a started task can report where it runs, not move.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SessionEnvironment {
    #[default]
    Local,
    Sandbox,
    Cloud,
}

impl SessionEnvironment {
    /// `skip_serializing_if` borrows — this takes `&self` for that.
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }

    pub fn is_sandbox(self) -> bool {
        matches!(self, Self::Sandbox)
    }

    pub fn is_cloud(self) -> bool {
        matches!(self, Self::Cloud)
    }

    /// The environments a draft may pick for `provider`: Cloud joins only
    /// when the provider has a hosted environment to submit to.
    pub fn options_for(provider: ProviderKind) -> Vec<Self> {
        let mut options = vec![Self::Local, Self::Sandbox];
        if provider.supports_cloud() {
            options.push(Self::Cloud);
        }
        options
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Local => "icons/laptop.svg",
            Self::Sandbox => "icons/container.svg",
            Self::Cloud => "icons/cloud-upload.svg",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ProviderModelOption {
    pub id: String,
    pub label: String,
    /// The i18n semantic behind `label`, when the daemon composed it from a
    /// known key rather than a provider name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_i18n: Option<crate::protocol::WireTranslation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The i18n semantic behind `description`, same contract as `label_i18n`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_i18n: Option<crate::protocol::WireTranslation>,
}

impl ProviderModelOption {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            label_i18n: None,
            description: None,
            description_i18n: None,
        }
    }

    /// A `localized!` pair supplies both the English label and its semantic.
    pub fn keyed(id: impl Into<String>, pair: (String, crate::protocol::WireTranslation)) -> Self {
        Self {
            label_i18n: Some(pair.1),
            ..Self::new(id, pair.0)
        }
    }

    /// An optional semantic for `label`, for sites where the pair itself is
    /// conditional. A `Some` pairs with the already-set label text.
    pub fn with_label_i18n(mut self, i18n: Option<crate::protocol::WireTranslation>) -> Self {
        self.label_i18n = i18n;
        self
    }

    /// A `localized!` pair for `description`, same contract as `keyed`.
    pub fn keyed_description(mut self, pair: (String, crate::protocol::WireTranslation)) -> Self {
        self.description = Some(pair.0);
        self.description_i18n = Some(pair.1);
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        let description = description.into();
        if !description.trim().is_empty() {
            self.description = Some(description);
        }
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ProviderModel {
    pub id: String,
    pub name: String,
    /// The i18n semantic behind `name`, when the daemon composed it from a
    /// known key rather than a provider name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_i18n: Option<crate::protocol::WireTranslation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_provider: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub reasoning_efforts: Vec<ProviderModelOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_reasoning_effort: Option<String>,
    #[serde(default)]
    pub service_tiers: Vec<ProviderModelOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_service_tier: Option<String>,
    /// Context window sizes the provider exposes as a per-session choice.
    /// Claude Code keeps its 1M window opt-in behind a model-id suffix, so the
    /// window is a trait of the session rather than of the model.
    #[serde(default)]
    pub context_windows: Vec<ProviderModelOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_context_window: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct FavoriteModel {
    pub provider: ProviderKind,
    pub model: String,
    /// The picker treats a favorite as a model+effort+fast selection, not a
    /// bare model. `None` predates combo favorites and resolves to the
    /// model's default effort at match time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default)]
    pub fast: bool,
}

/// One provider-owned agent composition available when a task starts.
///
/// DeepSeek Harness calls these agent presets. A preset chooses the tools and
/// prompt composition for a session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ProviderAgentPreset {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub is_custom: bool,
}

impl ProviderAgentPreset {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            is_default: false,
            is_custom: false,
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        let description = description.into();
        if !description.trim().is_empty() {
            self.description = Some(description);
        }
        self
    }

    pub fn default(mut self) -> Self {
        self.is_default = true;
        self
    }

    /// Harness localizes its four shipped presets in the Web client rather
    /// than in the Host roster, whose metadata may use the install language.
    /// Mirror that boundary while leaving user-authored metadata untouched.
    pub fn display_name(&self) -> String {
        if !self.is_custom {
            match self.id.as_str() {
                "standard" => return tr!("agent_preset.standard"),
                "code" => return tr!("agent_preset.code"),
                "minimal" => return tr!("agent_preset.minimal"),
                "cordis" => return tr!("agent_preset.creator"),
                _ => {}
            }
        }
        self.name.clone()
    }

    pub fn display_description(&self) -> Option<String> {
        if !self.is_custom {
            match self.id.as_str() {
                "standard" => return Some(tr!("agent_preset.standard_description")),
                "code" => return Some(tr!("agent_preset.code_description")),
                "minimal" => return Some(tr!("agent_preset.minimal_description")),
                "cordis" => return Some(tr!("agent_preset.creator_description")),
                _ => {}
            }
        }
        self.description.clone()
    }
}

/// A named subagent a session's model can delegate work to.
///
/// Definitions are injected at launch through whatever channel the harness
/// offers — Claude's `--agents` JSON, an OpenCode `agent.*` config entry, a
/// Pi extension tool — so the fields stay harness-neutral. The `waku-` name
/// prefix doubles as the attribution key: a `BackgroundWorkItem.role` that
/// starts with it identifies a Goddard-defined agent with no extra plumbing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct SubagentDef {
    /// Agent name as the harness reports it, conventionally `waku-<name>`.
    pub name: String,
    /// "When to use" text the harness surfaces to the orchestrating model.
    pub description: String,
    /// The subagent's instructions.
    pub prompt: String,
    /// Read-only agents have write tools denied where the harness allows it.
    #[serde(default)]
    pub read_only: bool,
    /// Harness-native model id; `None` runs the harness's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Harness-native reasoning effort, where the harness supports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// The subagents a launch injects. Every field is launch-time only — no
/// harness can re-inject mid-session.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct SubagentSpec {
    #[serde(default)]
    pub agents: Vec<SubagentDef>,
}

impl ProviderModel {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            name_i18n: None,
            sub_provider: None,
            is_default: false,
            reasoning_efforts: Vec::new(),
            default_reasoning_effort: None,
            service_tiers: Vec::new(),
            default_service_tier: None,
            context_windows: Vec::new(),
            default_context_window: None,
        }
    }

    /// A `localized!` pair supplies both the English name and its semantic.
    pub fn keyed(id: impl Into<String>, pair: (String, crate::protocol::WireTranslation)) -> Self {
        Self {
            name_i18n: Some(pair.1),
            ..Self::new(id, pair.0)
        }
    }

    pub fn default(mut self) -> Self {
        self.is_default = true;
        self
    }

    pub fn sub_provider(mut self, sub_provider: impl Into<String>) -> Self {
        self.sub_provider = Some(sub_provider.into());
        self
    }

    pub fn reasoning(
        mut self,
        efforts: impl IntoIterator<Item = ProviderModelOption>,
        default: impl Into<String>,
    ) -> Self {
        self.reasoning_efforts = efforts.into_iter().collect();
        self.default_reasoning_effort = Some(default.into());
        self
    }

    pub fn service_tiers(
        mut self,
        tiers: impl IntoIterator<Item = ProviderModelOption>,
        default: impl Into<String>,
    ) -> Self {
        self.service_tiers = tiers.into_iter().collect();
        self.default_service_tier = Some(default.into());
        self
    }

    pub fn context_windows(
        mut self,
        windows: impl IntoIterator<Item = ProviderModelOption>,
        default: impl Into<String>,
    ) -> Self {
        self.context_windows = windows.into_iter().collect();
        self.default_context_window = Some(default.into());
        self
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct ProviderProbe {
    pub provider: ProviderKind,
    pub installed: bool,
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub models: Vec<ProviderModel>,
    #[serde(default)]
    pub agent_presets: Vec<ProviderAgentPreset>,
}

impl ProviderProbe {
    pub fn preferred_model(&self) -> Option<&ProviderModel> {
        self.models
            .iter()
            .find(|model| model.is_default)
            .or_else(|| self.models.first())
    }

    pub fn model(&self, requested: &str) -> Option<&ProviderModel> {
        crate::model_catalog::packed_catalog_model(&self.models, requested, self.provider)
            .map(|matched| matched.model)
    }

    pub fn preferred_agent_preset(&self) -> Option<&ProviderAgentPreset> {
        self.agent_presets
            .iter()
            .find(|preset| preset.is_default)
            .or_else(|| self.agent_presets.first())
    }
}

pub fn parse_cli_version(output: &str) -> Option<String> {
    let line = output.lines().find(|line| !line.trim().is_empty())?;
    line.split_whitespace()
        .map(|token| {
            token
                .trim_start_matches('v')
                .trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-'))
        })
        .find(|token| {
            let mut parts = token.split('.');
            let leading_number = parts
                .next()
                .is_some_and(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
            leading_number
                && parts
                    .next()
                    .is_some_and(|part| part.chars().next().is_some_and(|c| c.is_ascii_digit()))
        })
        .map(str::to_owned)
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub path: PathBuf,
    /// Finder-bookmark data that re-resolves the folder after a rename or
    /// same-volume move. `None` where the platform API is unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<Vec<u8>>,
    /// When the project was added, unix seconds.
    #[serde(default)]
    pub created_at: u64,
    /// Picked ad hoc for one task ("New task in…") rather than registered as
    /// a project. Temporary projects leave the catalog once no live session
    /// references them; `false` for every project persisted before the flag
    /// existed.
    #[serde(default)]
    pub temporary: bool,
    /// Starred projects lead the ⌘D next-completion navigation — even an
    /// already-seen idle task in one outranks an unread completion elsewhere —
    /// and hoist above unstarred projects in the sidebar's Project grouping.
    #[serde(default)]
    pub starred: bool,
}

/// Filesystem context a task runs in.
///
/// Drafts may carry [`Self::NewWorktree`] until their first prompt. Goddard then
/// creates the Git worktree and replaces it with [`Self::Worktree`] before any
/// checkpoint or provider process can observe the task.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum SessionWorkspace {
    /// Work directly in the project's ordinary checkout.
    #[default]
    Local,
    /// Create an isolated worktree when this draft is first submitted. A
    /// selected base branch is remembered without checking it out in the
    /// ordinary project directory.
    NewWorktree {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_branch: Option<String>,
    },
    /// A materialized worktree. `path` preserves a project that points at a
    /// subdirectory of its repository rather than the repository root itself.
    /// `name` is the worktree's directory name and the session's workspace
    /// label; state persisted before names existed backfills it from `path`.
    Worktree {
        path: PathBuf,
        #[serde(default)]
        name: String,
        /// Branch last seen checked out in the worktree. `None` while it
        /// remains in the detached HEAD state it was created with.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        /// The base the worktree was created from — where `Land` sends its
        /// commits. `None` for sessions persisted before it was recorded or
        /// for worktrees that adopted a checkout's state.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_branch: Option<String>,
    },
}

impl SessionWorkspace {
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }

    pub fn is_worktree(&self) -> bool {
        matches!(self, Self::NewWorktree { .. } | Self::Worktree { .. })
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Worktree { path, .. } => Some(path),
            Self::Local | Self::NewWorktree { .. } => None,
        }
    }

    /// State persisted before worktrees had names deserializes `name` as
    /// empty; derive it from the worktree's directory.
    pub fn backfill_worktree_name(&mut self) {
        if let Self::Worktree { path, name, .. } = self
            && name.is_empty()
            && let Some(directory) = path.file_name()
        {
            *name = directory.to_string_lossy().into_owned();
        }
    }
}

impl Project {
    pub const PROJECTLESS_NAME: &'static str = "No project";

    pub fn display_name(&self) -> String {
        if self.is_projectless() {
            tr!("project.no_project_name")
        } else {
            self.name.clone()
        }
    }

    pub fn from_path(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("Project")
            .to_owned();
        Self {
            id: Uuid::new_v4(),
            name,
            path,
            bookmark: None,
            created_at: unix_time(),
            temporary: false,
            starred: false,
        }
    }

    pub fn is_projectless(&self) -> bool {
        crate::projectless::is_projectless_path(&self.path)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    #[default]
    Idle,
    Connecting,
    Working,
    Waiting,
    /// The turn is parked: the provider's reply ended, but detached work it
    /// will wake the session for is still running — Claude Code re-enters the
    /// model with a task notification once a backgrounded command, subagent
    /// or monitor settles. The turn stays open for that wake. Busy, but the
    /// provider is idle, so a new message steers straight in rather than
    /// waiting in the follow-up queue.
    Background,
    Failed,
}

impl SessionStatus {
    pub fn is_busy(self) -> bool {
        matches!(
            self,
            Self::Connecting | Self::Working | Self::Waiting | Self::Background
        )
    }
}

/// Who parked a queued follow-up. Composer-queued entries belong to the
/// client holding them — it edits, steers, removes, and drains them.
/// `Agent` entries mirror the daemon's `AgentPrompt` queue: the daemon owns
/// delivery and removal, and `sent_by` carries the same provenance as
/// [`Message::sent_by_task`] (`None` for automation runs and master-token
/// requests).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum QueuedMessageSource {
    #[default]
    User,
    Agent {
        sent_by: Option<Uuid>,
    },
}

/// A follow-up message queued while the agent is busy. It becomes its own
/// turn once the current turn settles successfully.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct QueuedMessage {
    pub id: Uuid,
    pub content: String,
    /// The text typed before Goddard appended provider-facing attachment
    /// mentions. `None` is the legacy/plain-message representation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<MessageAttachment>,
    /// Provider-facing text that renders no queued chip or transcript row —
    /// the internal "continue" nudge parked behind a busy session.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    /// Which queue owns the entry. Absent in documents written before the
    /// field existed — those are all composer-owned, so `User` is default.
    #[serde(default, skip_serializing_if = "QueuedMessageSource::is_user")]
    pub source: QueuedMessageSource,
    pub created_at: u64,
}

impl QueuedMessageSource {
    fn is_user(&self) -> bool {
        matches!(self, Self::User)
    }
}

impl QueuedMessage {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            content: content.into(),
            display_content: None,
            attachments: Vec::new(),
            hidden: false,
            source: QueuedMessageSource::User,
            created_at: unix_time(),
        }
    }

    /// The daemon's mirror of a parked agent prompt: it renders a queued
    /// chip with provenance but is delivered and removed only by the daemon.
    pub fn agent(content: impl Into<String>, sent_by: Option<Uuid>) -> Self {
        Self {
            source: QueuedMessageSource::Agent { sent_by },
            ..Self::new(content)
        }
    }

    pub fn with_presentation(
        content: impl Into<String>,
        display_content: Option<String>,
        attachments: Vec<MessageAttachment>,
    ) -> Self {
        Self {
            display_content,
            attachments,
            ..Self::new(content)
        }
    }

    pub fn visible_content(&self) -> &str {
        self.display_content.as_deref().unwrap_or(&self.content)
    }

    /// Daemon-owned entries wait for the daemon's queue drain; the client
    /// holding the session must not submit, edit, or locally remove them.
    pub fn is_agent_owned(&self) -> bool {
        matches!(self.source, QueuedMessageSource::Agent { .. })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TurnStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CheckpointStatus {
    Ready,
    Unavailable,
    Error,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CheckpointFile {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct Checkpoint {
    pub turn_count: usize,
    pub git_ref: String,
    pub status: CheckpointStatus,
    #[serde(default)]
    pub files: Vec<CheckpointFile>,
    /// Cached once at capture time so a visible transcript row never walks a
    /// potentially huge file list on every frame.
    #[serde(default)]
    pub additions: u64,
    #[serde(default)]
    pub deletions: u64,
    pub created_at: u64,
}

impl Checkpoint {
    pub fn refresh_totals(&mut self) {
        self.additions = self.files.iter().map(|file| file.additions).sum();
        self.deletions = self.files.iter().map(|file| file.deletions).sum();
    }

    pub fn totals_are_current(&self) -> bool {
        self.additions == self.files.iter().map(|file| file.additions).sum::<u64>()
            && self.deletions == self.files.iter().map(|file| file.deletions).sum::<u64>()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct AgentTurn {
    pub id: Uuid,
    pub turn_count: usize,
    pub status: TurnStatus,
    #[serde(default)]
    pub provider_turn_started: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_resume_at: Option<String>,
    pub started_at: u64,
    pub completed_at: Option<u64>,
    #[serde(default)]
    pub checkpoint: Option<Checkpoint>,
}

/// How full the provider's context window is, from the latest main-thread
/// model call. `tokens` is prompt + cache + output of that call; `window` is
/// the model's context size, which the provider only reports once a turn
/// settles — `None` means "not known yet", and the meter degrades to a bare
/// token count.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize, TS)]
pub struct ContextUsage {
    pub tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
}

/// Where Codex stands on a thread goal. Mirrors the app-server's
/// `ThreadGoalStatus` vocabulary; the serialized names are Codex's own so a
/// status can round-trip through `thread/goal/set` unchanged.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ThreadGoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

impl ThreadGoalStatus {
    /// Whether the goal is still being pursued or can be resumed. Complete
    /// and budget-limited goals are terminal: replacing them starts fresh
    /// accounting instead of continuing the old ledger.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Complete | Self::BudgetLimited)
    }
}

/// Why a provider-session catalog came back empty — distinguishes a
/// genuinely empty history from a provider that cannot enumerate one.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProviderSessionCatalogStatus {
    #[default]
    Ready,
    /// The agent cannot list past sessions (e.g. an ACP agent without the
    /// `session/list` capability, or a provider with no readable store).
    Unsupported,
    /// The provider's CLI wasn't found on the daemon host, so no catalog
    /// could even be attempted.
    BinaryMissing,
}

/// A resumable conversation discovered in a provider CLI's own history.
///
/// This is deliberately lightweight: the command palette can list hundreds of
/// native sessions without moving their transcripts over the daemon protocol.
/// [`ProviderSessionHistory`] is fetched only after the user chooses one.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ProviderSessionSummary {
    pub cursor: ProviderResumeCursor,
    pub title: String,
    pub cwd: PathBuf,
    /// The recorded working directory no longer exists on the daemon host;
    /// resuming falls back to its nearest surviving ancestor.
    #[serde(default)]
    pub cwd_missing: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

impl ProviderSessionSummary {
    pub fn provider(&self) -> ProviderKind {
        self.cursor.provider()
    }
}

/// The displayable portion of a provider-native conversation imported into a
/// Goddard task. Provider history remains authoritative; unsupported native
/// items such as private reasoning or provider-only control records are
/// intentionally absent.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
pub struct ProviderSessionHistory {
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub turns: Vec<AgentTurn>,
}

/// A provider-persisted objective the agent keeps pursuing across turns.
/// Field names follow the Codex app-server payload so its `goal` objects
/// deserialize directly.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGoal {
    pub objective: String,
    pub status: ThreadGoalStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<i64>,
    #[serde(default)]
    pub tokens_used: i64,
    #[serde(default)]
    pub time_used_seconds: i64,
}

/// A goal mutation the client asks the provider runtime to perform. Results
/// come back asynchronously as [`DriverEvent::GoalUpdated`]; failures surface
/// through [`DriverEvent::Error`].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GoalOperation {
    /// Re-read the provider's current goal without changing it.
    Refresh,
    /// Create or update the goal. `None` fields keep their provider-side
    /// value; `replace` clears the existing goal first so a new objective
    /// starts with fresh token and time accounting.
    Set {
        objective: Option<String>,
        status: Option<ThreadGoalStatus>,
        replace: bool,
    },
    Clear,
}

/// Last daemon event incorporated into a session's persisted projection.
///
/// The daemon runtime and its replay journal can outlive any particular
/// desktop or browser connection. Persisting this cursor with the transcript
/// lets a newly attached client replay only the events the stored projection
/// has not already applied.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RuntimeEventCursor {
    pub runtime_id: Uuid,
    pub epoch: Uuid,
    pub sequence: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct AgentSession {
    pub id: Uuid,
    /// A title explicitly chosen by the user. [`Self::DEFAULT_TITLE`] means
    /// no explicit title has been set, so [`Self::auto_title`] may be shown.
    pub title: String,
    /// Best-effort title supplied by the provider, or derived locally from the
    /// first prompt until the provider reports a better one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_title: Option<String>,
    pub project_id: Uuid,
    /// Local project checkout or an isolated Git worktree for this task.
    #[serde(default, skip_serializing_if = "SessionWorkspace::is_local")]
    pub workspace: SessionWorkspace,
    /// The checkout the session ran in before moving into a worktree. While
    /// `Some`, the next outbound prompt prepends a one-shot note that the
    /// working directory changed — the resumed thread's context still names
    /// the old checkout's paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_moved_from: Option<PathBuf>,
    /// When `Some`, this session is a side chat spawned from the named
    /// parent task. Side chats are hidden from task lists, opened in the
    /// parent's right panel, and deleted when the parent is archived or
    /// removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_chat_of: Option<Uuid>,
    pub provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub runtime_mode: RuntimeMode,
    /// Where the task's work runs — this Mac, the sandbox VM, or the
    /// provider's hosted cloud. Fixed when the session boots — a started
    /// task can report where it runs, not move. Read through
    /// [`Self::environment`], which folds in the legacy `sandboxed` flag.
    #[serde(default, skip_serializing_if = "SessionEnvironment::is_local")]
    pub environment: SessionEnvironment,
    /// Read-only compatibility field for state written before
    /// `environment` existed; new saves omit it. Never read directly — use
    /// [`Self::environment`].
    #[serde(default, skip_serializing)]
    pub sandboxed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    /// Selected context window, when the provider exposes more than one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<String>,
    /// Provider-owned agent composition selected before the first turn.
    /// Currently populated by DeepSeek Harness, which locks this value once
    /// conversation history exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_preset: Option<String>,
    /// The draft's model selection is Auto: the first submission routes the
    /// task through the evaluation router instead of starting `provider`
    /// directly. Meaningless once the session has started — `route_decision`
    /// is the record of what routing chose.
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_route: bool,
    /// The routing decision that produced this session's provider and model.
    /// Present only on sessions that started through Auto.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_decision: Option<RouteDecision>,
    /// Where the task sits in a plan-then-execute lifecycle — `None` until
    /// the tool stream produces a signal worth classifying. Phase routing
    /// reads it for the sidebar marker and the downshift decision; a rewind
    /// re-derives it from what survives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<crate::routing::SessionPhase>,
    pub status: SessionStatus,
    pub created_at: u64,
    /// Any mutation, including title edits and truncation. Use
    /// [`Self::last_reply_at`] for conversation recency.
    pub updated_at: u64,
    /// Activity time of the newest turn. Set as soon as the user submits it,
    /// then refreshed when the turn settles, whatever its outcome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reply_at: Option<u64>,
    /// When the session was archived, unix seconds. `None` while the session
    /// is active. Archived sessions are hidden from task lists and search,
    /// and are purged entirely once the archive outlives its retention
    /// window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<u64>,
    /// When the session was pinned to the top of the sidebar, unix seconds.
    /// `None` while the session sits in its ordinary group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<u64>,
    /// When the session was swept into the sidebar's Dormant group, unix
    /// seconds. The sweep is active only while this is the session's newest
    /// mutation — any later update wakes it. Stale sessions can also group
    /// as dormant without a flag; see the sidebar's dormancy rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dormant_at: Option<u64>,
    /// Auto-dormancy is suppressed until this time, unix seconds. A manual
    /// restore snoozes the stale-session sweep for one threshold period so a
    /// still-stale session does not fold straight back into Dormant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dormant_exempt_until: Option<u64>,
    /// Received-file sessions start quarantined: the transfer's files sit in
    /// the workspace untouched until the user explicitly trusts them, and
    /// the daemon refuses prompts while this is set.
    #[serde(default, skip_serializing_if = "is_false")]
    pub quarantined: bool,
    /// When the session's workspace landed on its base branch, unix seconds.
    /// `None` while the session's work has not been landed through the app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landed_at: Option<u64>,
    /// Incognito sessions are held in memory only: the daemon never writes
    /// them to its store, never injects project memory, and never feeds them
    /// to distillation. The flag is fixed at creation — a session that has
    /// already persisted rows cannot be made incognito retroactively.
    /// A connected client keeps the only copy and re-registers it after a
    /// daemon restart, so the session can still resume through
    /// `provider_cursor`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub incognito: bool,
    /// The user has allowed this task's agent to set its own title.
    #[serde(default, skip_serializing_if = "is_false")]
    pub agent_rename_allowed: bool,
    #[serde(default)]
    pub provider_cursor: Option<ProviderResumeCursor>,
    /// Provider conversations this session ran on before switching away.
    /// Each holds a resumable cursor and the transcript boundary the return
    /// compacts from — the current provider's entry lives in
    /// `provider`/`provider_cursor`, not here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suspended_provider_sessions: Vec<SuspendedProviderSession>,
    /// Compacted context staged by a provider switch, prepended to the next
    /// outbound prompt and then cleared — the same one-shot delivery
    /// `workspace_moved_from` uses. Persisted so a quit between the switch
    /// and the next prompt does not lose it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_provider_context: Option<String>,
    /// Slash commands the provider reported for this session's live process,
    /// kept so a resumed session still completes them before its next
    /// handshake.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_commands: Vec<ReportedCommand>,
    /// The provider-persisted goal for this session's thread, kept so a
    /// resumed session shows its goal before the runtime reconnects.
    /// Currently populated by Codex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_goal: Option<ThreadGoal>,
    /// Context-window occupancy from the live stream, kept so a resumed
    /// session's meter starts where the conversation left off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_usage: Option<ContextUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_event_cursor: Option<RuntimeEventCursor>,
    /// Read-only compatibility field for v1 state files. New saves omit it.
    #[serde(default, skip_serializing)]
    pub provider_session_id: Option<String>,
    /// Not stored in the session JSON — these are rows in the `messages`
    /// table, reattached when the session is hydrated.
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub transcript_blocks: Vec<TranscriptBlock>,
    #[serde(default)]
    pub turns: Vec<AgentTurn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queued_messages: Vec<QueuedMessage>,
    /// Whether the transcript has been read from the database.
    ///
    /// Startup loads only the columns the session list needs, so a session
    /// begins as a skeleton with empty `messages`, `transcript_blocks` and
    /// `turns`. Those are empty because nothing fetched them, not because the
    /// session is empty — never persist a skeleton, and never conclude from one
    /// that a session has no history. The flag crosses the wire so the daemon
    /// can tell a list projection — whose detail fields are placeholders —
    /// apart from a genuinely empty loaded session.
    #[serde(default = "detail_loaded_default", skip_serializing_if = "is_true")]
    pub detail_loaded: bool,
}

/// Anything deserialized from a `data` blob carries its full detail.
fn detail_loaded_default() -> bool {
    true
}

/// `skip_serializing_if` predicate for flags that omit themselves when off —
/// keeps old payloads legible to older readers.
pub fn is_false(value: &bool) -> bool {
    !*value
}

/// The inverse of [`is_false`], for flags that omit themselves when on.
fn is_true(value: &bool) -> bool {
    *value
}

/// Context blocks the daemon prepends to a session's first visible prompt —
/// `<project-map>`, then `<project-memory>`, then `<goddard-agent>`. They are
/// provider-facing context,
/// not user text, but a provider can still report them back as a title (Kimi
/// echoes the prompt verbatim; Devin's stored placeholder can truncate inside
/// a block), so anything deriving a title from prompt text drops them first.
/// An opener whose closer was truncated away is removed only when nothing
/// precedes it — a mid-title mention is real text.
pub fn strip_injected_prompt_blocks(text: &str) -> String {
    let mut cleaned = text.to_owned();
    loop {
        let before = cleaned.len();
        for tag in ["project-map", "project-memory", "goddard-agent"] {
            let open = format!("<{tag}>");
            let close = format!("</{tag}>");
            while let Some(start) = cleaned.find(&open) {
                match cleaned[start..].find(&close) {
                    Some(end) => cleaned.replace_range(start..start + end + close.len(), ""),
                    None if cleaned[..start].trim().is_empty() => {
                        cleaned.truncate(start);
                        break;
                    }
                    None => break,
                }
            }
        }
        if cleaned.len() == before {
            return cleaned.trim().to_owned();
        }
    }
}

impl AgentSession {
    pub const DEFAULT_TITLE: &'static str = "New task";

    pub fn new(project_id: Uuid, provider: ProviderKind) -> Self {
        let now = unix_time();
        Self {
            id: Uuid::new_v4(),
            title: Self::DEFAULT_TITLE.to_owned(),
            auto_title: None,
            project_id,
            workspace: SessionWorkspace::Local,
            workspace_moved_from: None,
            side_chat_of: None,
            provider,
            model: None,
            runtime_mode: RuntimeMode::default(),
            environment: SessionEnvironment::Local,
            sandboxed: false,
            reasoning_effort: None,
            service_tier: None,
            context_window: None,
            agent_preset: None,
            auto_route: false,
            route_decision: None,
            phase: None,
            status: SessionStatus::Idle,
            created_at: now,
            updated_at: now,
            last_reply_at: None,
            archived_at: None,
            pinned_at: None,
            dormant_at: None,
            dormant_exempt_until: None,
            quarantined: false,
            landed_at: None,
            incognito: false,
            agent_rename_allowed: false,
            detail_loaded: true,
            provider_cursor: None,
            suspended_provider_sessions: Vec::new(),
            pending_provider_context: None,
            available_commands: Vec::new(),
            thread_goal: None,
            context_usage: None,
            runtime_event_cursor: None,
            provider_session_id: None,
            messages: Vec::new(),
            transcript_blocks: Vec::new(),
            turns: Vec::new(),
            queued_messages: Vec::new(),
        }
    }

    /// Returns the lightweight projection used by task lists.
    ///
    /// A daemon can hold hydrated sessions in memory, but catalog refreshes
    /// must never clone or transmit their transcripts. Clients hydrate one
    /// selected session explicitly when they need its detail.
    pub fn list_projection(&self) -> Self {
        Self {
            id: self.id,
            title: self.title.clone(),
            auto_title: self.auto_title.clone(),
            project_id: self.project_id,
            // A list column, not detail: rows render worktree badges and
            // branch labels from it before the session is ever opened.
            workspace: self.workspace.clone(),
            workspace_moved_from: None,
            // List consumers need the link: it is how they know to keep the
            // row out of the task list.
            side_chat_of: self.side_chat_of,
            provider: self.provider,
            model: self.model.clone(),
            runtime_mode: RuntimeMode::default(),
            environment: self.environment(),
            sandboxed: false,
            reasoning_effort: None,
            service_tier: None,
            context_window: None,
            agent_preset: None,
            auto_route: self.auto_route,
            route_decision: self.route_decision.clone(),
            // A sidebar row shows the phase before the session is opened.
            phase: self.phase,
            status: self.status,
            created_at: self.created_at,
            updated_at: self.updated_at,
            last_reply_at: self.last_reply_at,
            archived_at: self.archived_at,
            pinned_at: self.pinned_at,
            dormant_at: self.dormant_at,
            dormant_exempt_until: self.dormant_exempt_until,
            quarantined: self.quarantined,
            landed_at: self.landed_at,
            // A list column: the sidebar and drafts list badge incognito rows.
            incognito: self.incognito,
            agent_rename_allowed: self.agent_rename_allowed,
            // Incognito sessions persist nowhere, so the client's skeleton
            // may be the only surviving copy after a daemon restart — it
            // must carry the resume cursor, or survival depends on whether
            // the session happened to be hydrated.
            provider_cursor: self
                .incognito
                .then(|| self.provider_cursor.clone())
                .flatten(),
            suspended_provider_sessions: Vec::new(),
            pending_provider_context: None,
            available_commands: Vec::new(),
            thread_goal: None,
            context_usage: None,
            runtime_event_cursor: None,
            provider_session_id: None,
            messages: Vec::new(),
            transcript_blocks: Vec::new(),
            turns: Vec::new(),
            queued_messages: Vec::new(),
            detail_loaded: false,
        }
    }

    /// The resolved run environment. State written before `environment`
    /// existed carries only the `sandboxed` bool — map it forward instead
    /// of silently unsandboxing those sessions.
    pub fn environment(&self) -> SessionEnvironment {
        if self.environment == SessionEnvironment::Local && self.sandboxed {
            SessionEnvironment::Sandbox
        } else {
            self.environment
        }
    }

    pub fn is_busy(&self) -> bool {
        self.status.is_busy()
    }

    /// Whether this session is a side chat bound to a parent task — hidden
    /// from task lists and deleted when the parent is archived or removed.
    pub fn is_side_chat(&self) -> bool {
        self.side_chat_of.is_some()
    }

    /// The provider-facing note a side chat prepends to its first outbound
    /// prompt: it names the parent task and explains how to reach it. The
    /// `read` surface is always in scope for a side chat when the daemon can
    /// deliver the CLI (`read_available`); `prompt` stays gated on the
    /// cross-task tools flag (`task_tools`). The transcript keeps the
    /// user's text; this rides the driver's prompt like the workspace-move
    /// notice. `None` once the first turn is behind it.
    pub fn side_chat_intro(
        &self,
        parent: &AgentSession,
        task_tools: bool,
        read_available: bool,
    ) -> Option<String> {
        if self.side_chat_of != Some(parent.id) || self.turns.len() > 1 {
            return None;
        }
        let mut intro = format!(
            "You are a side chat of the Goddard task \"{}\" (task id {}). \
             Its transcript is not in your context.",
            parent.display_title(),
            parent.id,
        );
        if read_available {
            intro.push_str(&format!(
                " Read it with `goddard-agent read '{{\"task_id\":\"{}\"}}'` when you \
                 need it",
                parent.id,
            ));
            if task_tools {
                intro.push_str(
                    ", and send it a message with `goddard-agent prompt` only when the \
                     user asks",
                );
            }
            intro.push('.');
        }
        Some(intro)
    }

    /// The flattening [`Self::agent_transcript`] and [`Self::transcript_index`]
    /// share: messages and condensed tool activity in reading order — blocks
    /// render after `after_message` messages, so a block precedes the message
    /// at its index — each item tagged with its 1-based turn number. Hidden
    /// provider-facing nudges and private reasoning are omitted.
    fn transcript_items(&self, turn: Option<usize>) -> Vec<AgentTranscriptItem> {
        let turn_numbers: std::collections::HashMap<Uuid, usize> = self
            .turns
            .iter()
            .map(|entry| (entry.id, entry.turn_count))
            .collect();
        let turn_of = |turn_id: Option<Uuid>| turn_id.and_then(|id| turn_numbers.get(&id)).copied();
        let in_turn = |turn_id: Option<Uuid>| match turn {
            Some(want) => turn_of(turn_id) == Some(want),
            None => true,
        };
        let mut items = Vec::new();
        for position in 0..=self.messages.len() {
            for block in &self.transcript_blocks {
                if block.after_message != position || !in_turn(block.turn_id) {
                    continue;
                }
                for activity in &block.activities {
                    if let Some(text) = activity.condensed_text(AGENT_TRANSCRIPT_ACTIVITY_CAP) {
                        items.push(AgentTranscriptItem {
                            turn: turn_of(block.turn_id),
                            kind: AgentTranscriptItemKind::Activity,
                            role: None,
                            content: text,
                        });
                    }
                }
            }
            let Some(message) = self.messages.get(position) else {
                continue;
            };
            if message.hidden || !in_turn(message.turn_id) {
                continue;
            }
            let content = message.visible_content().trim();
            if content.is_empty() {
                continue;
            }
            items.push(AgentTranscriptItem {
                turn: turn_of(message.turn_id),
                kind: AgentTranscriptItemKind::Message,
                role: Some(message.role),
                content: truncate_chars(content, AGENT_TRANSCRIPT_MESSAGE_CAP),
            });
        }
        items
    }

    /// The compact transcript `goddard-agent read` hands to a scoped agent
    /// caller. Entry text is capped per item and the listing is capped in
    /// total — when the total cap drops the oldest entries the transcript
    /// reports `truncated`, and per-turn reads reach what fell off.
    pub fn agent_transcript(&self, turn: Option<usize>) -> AgentSessionTranscript {
        let mut items = self.transcript_items(turn);
        // The total cap bounds the listing only — a per-turn read must
        // reach exactly the content the capped listing dropped.
        let mut truncated = false;
        if turn.is_none() {
            let mut size = items.iter().map(|item| item.content.len()).sum::<usize>();
            while size > AGENT_TRANSCRIPT_TOTAL_CAP && !items.is_empty() {
                size -= items.remove(0).content.len();
                truncated = true;
            }
        }
        AgentSessionTranscript {
            task_id: self.id,
            title: self.display_title().to_owned(),
            provider: self.provider,
            status: self.status,
            items,
            truncated,
        }
    }

    /// The per-turn index a handoff injects: each turn's user text verbatim
    /// (capped per entry) plus one extractive cue line per other entry — an
    /// activity's condensed title, a reply's first line — capped per turn.
    /// These are pointers into `goddard-agent read`, never a summary, and
    /// the listing cap does not apply: an index must name every turn.
    ///
    /// Returns `(turn, lines)` groups in first-seen order; `None` groups
    /// hold content outside a turn (markers, legacy rows).
    pub fn transcript_index(&self) -> Vec<(Option<usize>, Vec<String>)> {
        let mut groups: Vec<(Option<usize>, Vec<String>, usize)> = Vec::new();
        let group = |groups: &mut Vec<(Option<usize>, Vec<String>, usize)>, turn: Option<usize>| {
            groups
                .iter()
                .position(|(group_turn, ..)| *group_turn == turn)
                .unwrap_or_else(|| {
                    groups.push((turn, Vec::new(), 0));
                    groups.len() - 1
                })
        };
        // Unturned entries — markers, legacy rows — index under the turn
        // they follow; a `turn` read stays strictly by `turn_id`, but a
        // pointer map is better served by proximity.
        let mut last_turn = None;
        for item in self.transcript_items(None) {
            let (line, counted) = match item.kind {
                AgentTranscriptItemKind::Message if item.role == Some(MessageRole::User) => (
                    format!(
                        "User: {}",
                        truncate_chars(&item.content, AGENT_INDEX_USER_CAP)
                    ),
                    false,
                ),
                AgentTranscriptItemKind::Message => {
                    let role = match item.role {
                        Some(MessageRole::Assistant) => "Assistant: ",
                        Some(MessageRole::System) => "System: ",
                        _ => "",
                    };
                    let first = item.content.lines().next().unwrap_or_default();
                    (
                        format!("— {role}{}", truncate_chars(first, AGENT_INDEX_CUE_CAP)),
                        true,
                    )
                }
                AgentTranscriptItemKind::Activity => {
                    let first = item.content.lines().next().unwrap_or_default();
                    (
                        format!("— {}", truncate_chars(first, AGENT_INDEX_CUE_CAP)),
                        true,
                    )
                }
            };
            let index = group(&mut groups, item.turn.or(last_turn));
            if item.turn.is_some() {
                last_turn = item.turn;
            }
            let (.., lines, cues) = &mut groups[index];
            if !counted || *cues < AGENT_INDEX_CUES_PER_TURN {
                *cues += usize::from(counted);
                lines.push(line);
            }
        }
        groups
            .into_iter()
            .map(|(turn, lines, ..)| (turn, lines))
            .collect()
    }

    /// Derives [`Self::last_reply_at`] from the turn history when it is not
    /// already known, so a session stored before the field existed still sorts
    /// and displays correctly.
    pub fn backfill_last_reply_at(&mut self) {
        if self.last_reply_at.is_some() {
            return;
        }
        self.last_reply_at = self
            .turns
            .last()
            .map(|turn| turn.completed_at.unwrap_or(turn.started_at))
            .filter(|_| self.has_started());
    }

    pub fn has_started(&self) -> bool {
        // A skeleton came from a stored row, and only started sessions are
        // stored, so it has started even though its transcript is not loaded.
        !self.detail_loaded
            || !self.turns.is_empty()
            || !self.messages.is_empty()
            || self.provider_cursor.is_some()
    }

    /// The one-shot working-directory note for the first prompt sent after a
    /// move into a worktree, or `None` when nothing is pending — or when the
    /// session is somehow not on a worktree, in which case the flag stays so
    /// a later, valid send still announces the move it recorded.
    pub fn take_workspace_move_notice(&mut self) -> Option<String> {
        let to = self.workspace.path()?.to_path_buf();
        let from = self.workspace_moved_from.take()?;
        Some(format!(
            "The working directory for this session moved to {}. The checkout \
             it ran in before, {}, still exists but is now a stale copy — \
             absolute paths recorded earlier in this conversation point \
             there. Read and write files only under the new directory.",
            to.display(),
            from.display()
        ))
    }

    /// Where the transcript stands now, recorded as a suspended provider's
    /// `boundary` when the session switches away from it.
    pub fn transcript_boundary(&self) -> TranscriptBoundary {
        TranscriptBoundary {
            messages: self.messages.len(),
            blocks: self.transcript_blocks.len(),
        }
    }

    /// The staged provider-switch context for the next outbound prompt, or
    /// `None` once consumed. Like [`Self::take_workspace_move_notice`] it is
    /// provider-facing only — the transcript keeps the user's text.
    pub fn take_provider_context(&mut self) -> Option<String> {
        self.pending_provider_context.take()
    }

    /// Drops the loaded transcript so the session returns to its skeleton
    /// state, releasing the heap its messages, blocks and turns occupied.
    ///
    /// A session must be fully persisted and unmodified before this runs —
    /// callers check the store's dirty set — because the released fields are
    /// gone until the next store `hydrate` reloads them. The session keeps
    /// its list columns and cursors, and a later save of the skeleton only
    /// touches those columns, never the untouched detail row.
    pub fn release_transcript(&mut self) {
        self.messages = Vec::new();
        self.transcript_blocks = Vec::new();
        self.turns = Vec::new();
        self.queued_messages = Vec::new();
        self.detail_loaded = false;
    }

    /// Identifier owned by the underlying agent CLI, once its native session
    /// has been established.
    pub fn provider_native_id(&self) -> Option<&str> {
        self.provider_cursor
            .as_ref()
            .map(ProviderResumeCursor::native_id)
            .filter(|id| !id.trim().is_empty())
    }

    pub fn display_title(&self) -> &str {
        if self.title != Self::DEFAULT_TITLE && !self.title.trim().is_empty() {
            &self.title
        } else {
            self.auto_title
                .as_deref()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or(Self::DEFAULT_TITLE)
        }
    }

    /// Sets the user-owned title shown ahead of any provider fallback.
    /// Empty names are rejected so a cancelled inline rename cannot hide the
    /// existing title. Returns whether the stored title changed.
    pub fn set_title(&mut self, title: impl AsRef<str>) -> bool {
        let title = title.as_ref().trim();
        if title.is_empty() || self.title == title {
            return false;
        }
        self.title = title.to_owned();
        self.updated_at = unix_time();
        true
    }

    pub fn set_title_from_prompt(&mut self, prompt: &str) {
        if self.messages.len() > 1 || self.title != Self::DEFAULT_TITLE || self.auto_title.is_some()
        {
            return;
        }
        let prompt = strip_injected_prompt_blocks(prompt);
        let mut title = prompt
            .split_whitespace()
            .take(7)
            .collect::<Vec<_>>()
            .join(" ");
        if !title.is_empty() {
            if title.chars().count() > 54 {
                title = format!("{}…", title.chars().take(53).collect::<String>());
            }
            self.auto_title = Some(title);
        }
    }

    /// Replaces the provider-owned title without disturbing an explicit user
    /// title. Returns whether the stored fallback changed.
    pub fn set_auto_title(&mut self, title: Option<String>) -> bool {
        let title = title.and_then(|title| {
            let title = strip_injected_prompt_blocks(&title);
            (!title.is_empty()).then_some(title)
        });
        if self.auto_title == title {
            return false;
        }
        self.auto_title = title;
        self.updated_at = unix_time();
        true
    }

    /// Whether this session has a provider conversation to preserve. Locally
    /// synthesized assistant messages — a transfer's delivery receipt, for
    /// example — do not count until the user actually prompts the session.
    pub fn provider_locked(&self) -> bool {
        // A skeleton cannot inspect its transcript; assume it is locked
        // rather than offering a provider switch the detail may disprove.
        !self.detail_loaded
            || self.provider_cursor.is_some()
            || self.provider_session_id.is_some()
            || self
                .messages
                .iter()
                .any(|message| message.role == MessageRole::User)
            || self.turns.iter().any(|turn| turn.provider_turn_started)
    }

    pub fn can_choose_model(&self, provider: ProviderKind) -> bool {
        // A different provider on a locked session routes through the
        // provider-switch flow rather than applying directly, so it needs a
        // loaded transcript to compact — skeletons stay same-provider only.
        !self.status.is_busy()
            && (self.provider == provider || !self.provider_locked() || self.detail_loaded)
    }

    pub fn migrate_legacy_state(&mut self) {
        self.workspace.backfill_worktree_name();
        if self.provider_cursor.is_none()
            && let Some(id) = self.provider_session_id.take()
        {
            self.provider_cursor = Some(ProviderResumeCursor::from_session_id(self.provider, id));
        }
        if self.provider == ProviderKind::Codex {
            for message in &mut self.messages {
                if message.role == MessageRole::Assistant && message.content.contains('\u{e200}') {
                    message.content = strip_legacy_codex_citations(&message.content);
                }
            }
        }
        let mut merged_blocks: Vec<TranscriptBlock> =
            Vec::with_capacity(self.transcript_blocks.len());
        for mut block in std::mem::take(&mut self.transcript_blocks) {
            if let Some(previous) = merged_blocks.last_mut()
                && previous.after_message == block.after_message
                && previous.turn_id == block.turn_id
            {
                previous.activities.append(&mut block.activities);
            } else {
                merged_blocks.push(block);
            }
        }
        self.transcript_blocks = merged_blocks;

        for block in &mut self.transcript_blocks {
            for activity in &mut block.activities {
                if activity.kind == ActivityKind::Search && activity.title.trim() == "Search for" {
                    // Persisted titles stay locale-neutral: stored English,
                    // not a tr! baked in whichever process migrated the row.
                    activity.title = "Browsed the web".to_owned();
                }
                let named_kind = ActivityKind::from_tool_name(&activity.title);
                if named_kind != ActivityKind::Tool
                    && matches!(
                        activity.kind,
                        ActivityKind::Search | ActivityKind::Tool | ActivityKind::FileChange
                    )
                {
                    activity.kind = named_kind;
                }
                if activity.arguments.is_none()
                    && activity.output.is_none()
                    && !activity.failed
                    && activity.detail.as_deref().is_some_and(|detail| {
                        serde_json::from_str::<serde_json::Value>(detail).is_ok()
                    })
                {
                    // Older provider transcripts stored input JSON in
                    // `detail`. Promote it once so it stays expandable but no
                    // longer floods the row preview.
                    activity.arguments = activity.detail.take();
                }
                activity.refresh_activity_metadata();
            }
        }

        // Checkpoints written before cached totals were added still have the
        // complete file list. Backfill once on load rather than making every
        // transcript frame rediscover the same totals.
        for turn in &mut self.turns {
            if let Some(checkpoint) = turn.checkpoint.as_mut() {
                checkpoint.refresh_totals();
            }
        }

        if !self.turns.is_empty()
            || !self
                .messages
                .iter()
                .any(|message| message.role == MessageRole::User)
        {
            return;
        }

        let user_indexes = self
            .messages
            .iter()
            .enumerate()
            .filter_map(|(index, message)| (message.role == MessageRole::User).then_some(index))
            .collect::<Vec<_>>();
        for (offset, start) in user_indexes.iter().copied().enumerate() {
            let end = user_indexes
                .get(offset + 1)
                .copied()
                .unwrap_or(self.messages.len());
            let id = Uuid::new_v4();
            let started_at = self.messages[start].created_at;
            let completed_at = self.messages[start..end]
                .iter()
                .map(|message| message.created_at)
                .max()
                .unwrap_or(started_at);
            for message in &mut self.messages[start..end] {
                message.turn_id = Some(id);
            }
            for block in &mut self.transcript_blocks {
                if block.after_message > start && block.after_message <= end {
                    block.turn_id = Some(id);
                }
            }
            self.turns.push(AgentTurn {
                id,
                turn_count: offset + 1,
                status: TurnStatus::Completed,
                provider_turn_started: true,
                provider_resume_at: None,
                started_at,
                completed_at: Some(completed_at),
                checkpoint: None,
            });
        }
    }

    #[doc(hidden)]
    pub fn begin_turn(&mut self, prompt: impl Into<String>) -> Uuid {
        self.begin_turn_with_presentation(prompt, None, Vec::new())
    }

    pub fn begin_turn_with_presentation(
        &mut self,
        prompt: impl Into<String>,
        display_content: Option<String>,
        attachments: Vec<MessageAttachment>,
    ) -> Uuid {
        self.begin_turn_inner(prompt, display_content, attachments, false)
    }

    /// Begin a turn whose prompt is provider-facing only — the internal
    /// nudge a "continue" sends to an interrupted session. The message stays
    /// in the record so every client's projection names the same rows, but
    /// no transcript row renders it.
    pub fn begin_hidden_turn(&mut self, prompt: impl Into<String>) -> Uuid {
        self.begin_turn_inner(prompt, None, Vec::new(), true)
    }

    fn begin_turn_inner(
        &mut self,
        prompt: impl Into<String>,
        display_content: Option<String>,
        attachments: Vec<MessageAttachment>,
        hidden: bool,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let now = unix_time();
        self.turns.push(AgentTurn {
            id,
            turn_count: self.turns.len() + 1,
            status: TurnStatus::Running,
            provider_turn_started: false,
            provider_resume_at: None,
            started_at: now,
            completed_at: None,
            checkpoint: None,
        });
        let mut prompt = Message::new_for_turn(MessageRole::User, prompt, id)
            .with_presentation(display_content, attachments);
        prompt.hidden = hidden;
        self.messages.push(prompt);
        self.last_reply_at = Some(now);
        id
    }

    /// Begin a turn the provider runs on its own — Codex goal continuation
    /// pursues an active goal whenever its thread is idle. There is no user
    /// message; the turn exists so the streamed output has a transcript home.
    /// Like a submission's turn it starts unconfirmed: the provider's own
    /// start report marks it via [`Self::mark_active_turn_provider_started`],
    /// and an unconfirmed one can be unwound if the pursuit never begins.
    pub fn begin_provider_turn(&mut self) -> Uuid {
        let id = Uuid::new_v4();
        let now = unix_time();
        self.turns.push(AgentTurn {
            id,
            turn_count: self.turns.len() + 1,
            status: TurnStatus::Running,
            provider_turn_started: false,
            provider_resume_at: None,
            started_at: now,
            completed_at: None,
            checkpoint: None,
        });
        self.last_reply_at = Some(now);
        self.updated_at = now;
        id
    }

    /// Mirror a prompt submitted to this session's runtime, possibly by
    /// another client.
    ///
    /// The submitting client already holds the turn and its user message, so
    /// a running turn that has a user message is left alone — that covers the
    /// submitter's own echo and a client that hydrated after the submission
    /// was saved. A running turn without one is a provider-started turn this
    /// client was following; the submission becomes its prompt. With no
    /// running turn the submission opens one here exactly as it did on the
    /// submitting client, reusing that client's ids so the projections every
    /// client saves agree on the rows. Returns whether the session changed.
    pub fn adopt_submitted_prompt(
        &mut self,
        message: &str,
        turn_id: Uuid,
        message_id: Uuid,
        sent_by_task: Option<Uuid>,
        hidden: bool,
    ) -> bool {
        let now = unix_time();
        // The daemon reuses a mirrored queue entry's id as the delivered
        // message's id, so a parked agent chip converts into this turn's
        // prompt even if its queue-change event was missed.
        let dequeued = self
            .queued_messages
            .iter()
            .any(|queued| queued.id == message_id);
        self.queued_messages
            .retain(|queued| queued.id != message_id);
        if let Some(active) = self.active_turn_id() {
            let has_prompt = self.messages.iter().any(|candidate| {
                candidate.turn_id == Some(active) && candidate.role == MessageRole::User
            });
            if has_prompt {
                return dequeued;
            }
            let mut prompt = Message::new_for_turn(MessageRole::User, message, active);
            prompt.id = message_id;
            prompt.sent_by_task = sent_by_task;
            prompt.hidden = hidden;
            self.messages.push(prompt);
            self.updated_at = now;
            return true;
        }
        if !hidden {
            self.set_title_from_prompt(message);
        }
        self.turns.push(AgentTurn {
            id: turn_id,
            turn_count: self.turns.len() + 1,
            status: TurnStatus::Running,
            provider_turn_started: false,
            provider_resume_at: None,
            started_at: now,
            completed_at: None,
            checkpoint: None,
        });
        let mut prompt = Message::new_for_turn(MessageRole::User, message, turn_id);
        prompt.id = message_id;
        prompt.sent_by_task = sent_by_task;
        prompt.hidden = hidden;
        self.messages.push(prompt);
        self.status = SessionStatus::Connecting;
        self.last_reply_at = Some(now);
        self.updated_at = now;
        true
    }

    /// Replace the daemon-owned slice of the follow-up queue with the
    /// daemon's latest snapshot. Composer-queued entries are untouched;
    /// combined order follows `created_at`. Returns whether anything changed.
    pub fn merge_agent_queued(&mut self, agent_messages: Vec<QueuedMessage>) -> bool {
        let mut combined: Vec<QueuedMessage> = self
            .queued_messages
            .iter()
            .filter(|queued| !queued.is_agent_owned())
            .cloned()
            .chain(agent_messages)
            .collect();
        combined.sort_by_key(|queued| queued.created_at);
        if combined == self.queued_messages {
            return false;
        }
        self.queued_messages = combined;
        self.updated_at = unix_time();
        true
    }

    /// Whether the running turn is a provider-initiated one whose pursuit has
    /// not been confirmed — no user message belongs to it and the provider
    /// has not reported its start. These are the optimistic turns a `/goal`
    /// begins, and the only turns safe to unwind on failure.
    pub fn active_turn_is_unconfirmed_pursuit(&self) -> bool {
        let Some(turn) = self
            .turns
            .last()
            .filter(|turn| turn.status == TurnStatus::Running && !turn.provider_turn_started)
        else {
            return false;
        };
        !self
            .messages
            .iter()
            .any(|message| message.turn_id == Some(turn.id))
    }

    pub fn active_turn_id(&self) -> Option<Uuid> {
        self.turns
            .last()
            .filter(|turn| turn.status == TurnStatus::Running)
            .map(|turn| turn.id)
    }

    /// Undo [`Self::begin_turn`] for a turn whose provider never started —
    /// the submission-preparation failure path, where the prompt returns to
    /// the composer, and a Continue retry of a settled turn whose prompt was
    /// never delivered. The turn and its messages leave the transcript, and a
    /// first-prompt unwind also gives back the default title that
    /// [`Self::set_title_from_prompt`] replaced. Its submission timestamp stays
    /// as the session's latest activity.
    pub fn unwind_unstarted_turn(&mut self, turn_id: Uuid) {
        let unstarted = self.turns.last().is_some_and(|turn| {
            turn.id == turn_id
                && !turn.provider_turn_started
                && matches!(
                    turn.status,
                    TurnStatus::Running | TurnStatus::Failed | TurnStatus::Interrupted
                )
        });
        if !unstarted {
            return;
        }
        self.turns.pop();
        self.messages
            .retain(|message| message.turn_id != Some(turn_id));
        if self.messages.is_empty() {
            self.auto_title = None;
        }
    }

    pub fn mark_active_turn_provider_started(&mut self) {
        if let Some(turn) = self
            .turns
            .last_mut()
            .filter(|turn| turn.status == TurnStatus::Running)
        {
            turn.provider_turn_started = true;
        }
    }

    pub fn mark_active_turn_provider_resume_at(&mut self, message_id: String) {
        if let Some(turn) = self
            .turns
            .last_mut()
            .filter(|turn| turn.status == TurnStatus::Running)
        {
            turn.provider_resume_at = Some(message_id);
        }
    }

    pub fn provider_turns_after(&self, turn_count: usize) -> usize {
        self.turns
            .iter()
            .skip(turn_count)
            .filter(|turn| turn.provider_turn_started)
            .count()
    }

    pub fn finish_active_turn(&mut self, status: TurnStatus) -> Option<(Uuid, usize)> {
        let turn = self
            .turns
            .last_mut()
            .filter(|turn| turn.status == TurnStatus::Running)?;
        let completed_at = unix_time();
        turn.status = status;
        turn.completed_at = Some(completed_at);
        let result = (turn.id, turn.turn_count);
        self.last_reply_at = Some(completed_at);
        Some(result)
    }

    pub fn push_message(&mut self, role: MessageRole, content: impl Into<String>) -> Uuid {
        let message = match self.active_turn_id() {
            Some(turn_id) => Message::new_for_turn(role, content, turn_id),
            None => Message::new(role, content),
        };
        let id = message.id;
        self.messages.push(message);
        id
    }

    /// Record a provider-facing user message no transcript row renders — a
    /// daemon-injected context steer the provider folded into the turn.
    /// The message stays in the record so the session documents the text the
    /// provider actually saw.
    pub fn push_hidden_user_message(&mut self, content: impl Into<String>) -> Uuid {
        let mut message = match self.active_turn_id() {
            Some(turn_id) => Message::new_for_turn(MessageRole::User, content, turn_id),
            None => Message::new(MessageRole::User, content),
        };
        message.hidden = true;
        let id = message.id;
        self.messages.push(message);
        id
    }

    /// [`push_message`] carrying a structured [`TranscriptNotice`] so clients
    /// draw the bespoke row instead of bare `content` text.
    pub fn push_notice_message(
        &mut self,
        role: MessageRole,
        content: impl Into<String>,
        notice: TranscriptNotice,
    ) -> Uuid {
        let mut message = match self.active_turn_id() {
            Some(turn_id) => Message::new_for_turn(role, content, turn_id),
            None => Message::new(role, content),
        };
        message.notice = Some(notice);
        let id = message.id;
        self.messages.push(message);
        id
    }

    pub fn push_user_message_with_presentation(
        &mut self,
        content: impl Into<String>,
        display_content: Option<String>,
        attachments: Vec<MessageAttachment>,
        sent_by_task: Option<Uuid>,
    ) -> Uuid {
        let mut message = match self.active_turn_id() {
            Some(turn_id) => Message::new_for_turn(MessageRole::User, content, turn_id),
            None => Message::new(MessageRole::User, content),
        }
        .with_presentation(display_content, attachments);
        message.sent_by_task = sent_by_task;
        let id = message.id;
        self.messages.push(message);
        id
    }

    pub fn truncate_after_turn(&mut self, turn_count: usize) {
        let retained = self
            .turns
            .iter()
            .take(turn_count)
            .map(|turn| turn.id)
            .collect::<std::collections::HashSet<_>>();
        self.turns.truncate(turn_count);
        self.messages.retain(|message| {
            message
                .turn_id
                .is_none_or(|turn_id| retained.contains(&turn_id))
        });
        self.transcript_blocks.retain(|block| {
            block
                .turn_id
                .is_none_or(|turn_id| retained.contains(&turn_id))
        });
        let message_count = self.messages.len();
        for block in &mut self.transcript_blocks {
            block.after_message = block.after_message.min(message_count);
        }
        self.rederive_phase();
        self.updated_at = unix_time();
    }

    /// Phase is derived state: a rewind can cut back across the
    /// planning→implementation boundary, so truncation recomputes it from
    /// the surviving activities rather than trusting what was recorded.
    fn rederive_phase(&mut self) {
        let mut saw_activity = false;
        let committing = self
            .transcript_blocks
            .iter()
            .flat_map(|block| block.activities.iter())
            .any(|activity| {
                saw_activity = true;
                activity.phase_signal() == crate::routing::PhaseSignal::Committing
            });
        self.phase = if committing {
            Some(crate::routing::SessionPhase::Executing)
        } else if saw_activity {
            Some(crate::routing::SessionPhase::Planning)
        } else {
            None
        };
    }

    pub fn fork_through_turn(
        &self,
        turn_count: usize,
        provider_cursor: ProviderResumeCursor,
        fork_title: &str,
    ) -> Option<Self> {
        if turn_count == 0 || turn_count > self.turns.len() {
            return None;
        }

        let mut fork = self.clone();
        fork.truncate_after_turn(turn_count);
        let fork_id = Uuid::new_v4();
        let turn_ids = fork
            .turns
            .iter()
            .map(|turn| (turn.id, Uuid::new_v4()))
            .collect::<std::collections::HashMap<_, _>>();

        for turn in &mut fork.turns {
            turn.id = turn_ids[&turn.id];
        }
        for message in &mut fork.messages {
            message.id = Uuid::new_v4();
            if let Some(turn_id) = message.turn_id {
                message.turn_id = turn_ids.get(&turn_id).copied();
            }
            message.streaming = false;
        }
        for block in &mut fork.transcript_blocks {
            if let Some(turn_id) = block.turn_id {
                block.turn_id = turn_ids.get(&turn_id).copied();
            }
        }

        let now = unix_time();
        fork.id = fork_id;
        fork.title = Self::DEFAULT_TITLE.to_owned();
        fork.agent_rename_allowed = false;
        fork.auto_title = Some(fork_title.to_owned());
        fork.status = SessionStatus::Idle;
        fork.created_at = now;
        fork.updated_at = now;
        fork.provider_cursor = Some(provider_cursor);
        fork.provider_session_id = None;
        // A fork snapshots the conversation, not the pending follow-ups its
        // source session is still holding for the live agent.
        fork.queued_messages.clear();
        Some(fork)
    }
}

fn strip_legacy_codex_citations(text: &str) -> String {
    const START: char = '\u{e200}';
    const END: char = '\u{e201}';
    const SEPARATOR: char = '\u{e202}';

    let mut remaining = text;
    let mut output = String::with_capacity(text.len());
    while let Some(start) = remaining.find(START) {
        output.push_str(&remaining[..start]);
        let marker_start = start + START.len_utf8();
        let Some(end_offset) = remaining[marker_start..].find(END) else {
            output.push_str(&remaining[start..]);
            return output;
        };
        let marker_end = marker_start + end_offset;
        let marker = &remaining[marker_start..marker_end];
        if marker
            .split(SEPARATOR)
            .next()
            .is_some_and(|prefix| prefix != "cite")
        {
            output.push_str(&remaining[start..marker_end + END.len_utf8()]);
        }
        remaining = &remaining[marker_end + END.len_utf8()..];
    }
    output.push_str(remaining);
    output
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

/// A file represented by a composer chip and retained with the sent message.
///
/// Render paths consume only this cached metadata; they never stat the file.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct MessageAttachment {
    /// Absolute path on the daemon host, handed to the provider. Clients must
    /// use `blob_reference` rather than opening this path themselves.
    pub path: PathBuf,
    /// Provider-facing path text, relative to the workspace when possible.
    pub mention: String,
    pub name: String,
    pub is_dir: bool,
    pub is_image: bool,
    /// Durable daemon-issued blob or attachment reference. The legacy field
    /// name is retained for storage compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_reference: Option<String>,
    /// Leading characters of a pasted-text attachment for the chip's hover
    /// preview. `Some` doubles as the pasted-text marker — clients render a
    /// "Pasted text" chip rather than a file tile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pasted_text_preview: Option<String>,
    /// When set, the attachment references another Goddard task rather than a
    /// file: `name` holds its title and `mention` its provider-facing token.
    /// `path`, `is_dir`, `is_image`, and `blob_reference` carry no file
    /// meaning in that case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
}

/// A structured transcript element persisted on a [`Message`]. `content`
/// always carries a plain-text rendering of the same event so clients that
/// predate a variant still show the pill; renderers that know it draw the
/// bespoke element instead.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum TranscriptNotice {
    /// The workspace's commits were landed on `base` — rebase-or-merge
    /// integration plus a base fast-forward. `commits` is newest-first and
    /// may be capped shorter than `ahead`, the true total.
    Landed {
        base: String,
        commits: Vec<CommitEntry>,
        ahead: u64,
    },
    /// A synthesized status line standing in for a reply the turn never
    /// produced ("Stopped", "Turn completed") or recording a session event
    /// ("Goal set"). `kind` picks the leading icon; `content` still carries
    /// the rendered text for clients that predate the variant.
    Status { kind: TranscriptNoticeStatus },
    /// The session moved to a different provider. `restarted` means the
    /// target's earlier provider session could not be resumed, so a fresh
    /// one was seeded with the full compacted history instead of the delta.
    ProviderSwitched {
        from: ProviderKind,
        to: ProviderKind,
        #[serde(default, skip_serializing_if = "is_false")]
        restarted: bool,
    },
}

/// Which icon a [`TranscriptNotice::Status`] row leads with.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TranscriptNoticeStatus {
    /// The user stopped the turn before the agent replied.
    Stopped,
    /// The turn finished cleanly without a reply.
    Completed,
    /// The turn failed before the agent replied.
    StoppedBeforeResponse,
    /// The provider ended the turn out of context.
    OutOfContext,
    /// The provider refused the turn.
    Declined,
    /// The provider stopped for a reported reason.
    StoppedWithReason,
    /// The provider process exited mid-turn.
    Exited,
    /// The agent runtime failed to start.
    StartFailed,
    /// Any other provider-reported failure.
    Error,
    /// A goal was set on the session.
    Goal,
}

/// A position in the persisted transcript: how many `messages` and
/// `transcript_blocks` existed when it was taken. The delta a suspended
/// provider missed is everything appended past its boundary.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct TranscriptBoundary {
    pub messages: usize,
    pub blocks: usize,
}

/// A provider-side conversation this session previously ran on, suspended
/// when it switched providers. `cursor` resumes it; `boundary` marks where
/// the transcript stood at suspension so a return can compact only the work
/// the provider never saw.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct SuspendedProviderSession {
    pub provider: ProviderKind,
    pub cursor: ProviderResumeCursor,
    pub boundary: TranscriptBoundary,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct Message {
    pub id: Uuid,
    #[serde(default)]
    pub turn_id: Option<Uuid>,
    pub role: MessageRole,
    pub content: String,
    /// Structured rendering of a system row; `None` for ordinary messages.
    /// Clients without the variant fall back to `content`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<TranscriptNotice>,
    /// User-visible text before provider-facing attachment mentions were
    /// appended. Plain and legacy messages omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<MessageAttachment>,
    /// The task whose agent submitted this message through the daemon's
    /// scoped agent commands. `None` for messages a human typed; every
    /// client renders the marker so agent-originated prompts stay visible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent_by_task: Option<Uuid>,
    /// Provider-facing text no client renders — the internal nudge a
    /// "continue" sends to an interrupted session. The message stays in the
    /// record so every projection carries the same ids.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    pub created_at: u64,
    pub streaming: bool,
}

impl Message {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            turn_id: None,
            role,
            content: content.into(),
            notice: None,
            display_content: None,
            attachments: Vec::new(),
            sent_by_task: None,
            hidden: false,
            created_at: unix_time(),
            streaming: false,
        }
    }

    pub fn new_for_turn(role: MessageRole, content: impl Into<String>, turn_id: Uuid) -> Self {
        Self {
            turn_id: Some(turn_id),
            ..Self::new(role, content)
        }
    }

    pub fn with_presentation(
        mut self,
        display_content: Option<String>,
        attachments: Vec<MessageAttachment>,
    ) -> Self {
        self.display_content = display_content;
        self.attachments = attachments;
        self
    }

    pub fn visible_content(&self) -> &str {
        self.display_content.as_deref().unwrap_or(&self.content)
    }
}

/// Per-entry and whole-listing bounds for `goddard-agent read`: the read
/// surface is for retrieval, so one pasted log or a long transcript must not
/// blow past what a scoped caller can usefully ingest.
const AGENT_TRANSCRIPT_MESSAGE_CAP: usize = 8 * 1024;
const AGENT_TRANSCRIPT_ACTIVITY_CAP: usize = 4 * 1024;
const AGENT_TRANSCRIPT_TOTAL_CAP: usize = 128 * 1024;

/// Bounds for [`AgentSession::transcript_index`]: the index is a map of
/// pointers, so one cue line and one turn's cue count stay small — the full
/// text is one `read` away.
const AGENT_INDEX_USER_CAP: usize = 2_000;
const AGENT_INDEX_CUE_CAP: usize = 320;
const AGENT_INDEX_CUES_PER_TURN: usize = 8;

/// Clip `text` to `cap` characters on a char boundary, marking the cut.
pub fn truncate_chars(text: &str, cap: usize) -> String {
    if text.chars().count() <= cap {
        return text.to_owned();
    }
    let mut clipped: String = text.chars().take(cap).collect();
    clipped.push_str(" […]");
    clipped
}

/// The compact transcript view [`crate::Command::AgentReadSession`] returns
/// to a scoped agent caller: enough of a task to answer questions about it,
/// with transport fields and provider internals stripped.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionTranscript {
    pub task_id: Uuid,
    pub title: String,
    pub provider: ProviderKind,
    pub status: SessionStatus,
    /// Transcript entries in reading order — messages and condensed tool
    /// activity. The oldest entries drop off when the listing exceeds its
    /// total cap; per-turn reads reach them.
    pub items: Vec<AgentTranscriptItem>,
    /// True when the total cap dropped entries from the front.
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

/// One entry in an [`AgentSessionTranscript`].
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentTranscriptItem {
    /// The 1-based turn number this entry belongs to — absent for entries
    /// outside a turn (system markers, pre-first-turn rows). `read`'s `turn`
    /// argument selects entries by this number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<usize>,
    pub kind: AgentTranscriptItemKind,
    /// The message role — present on `message` items only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<MessageRole>,
    /// Message text, or a condensed tool-activity line for `activity` items.
    pub content: String,
}

/// One hit [`crate::Command::AgentSearchSessions`] returns to a scoped agent
/// caller: enough of a matching task to decide whether its transcript is
/// worth reading in full.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSearchHit {
    pub task_id: Uuid,
    pub title: String,
    pub provider: ProviderKind,
    pub status: SessionStatus,
    pub updated_at: u64,
    /// Which side of the conversation `snippet` came from.
    pub source: MessageRole,
    /// The matched message text excerpted around the query, like the
    /// command palette shows.
    pub snippet: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentTranscriptItemKind {
    Message,
    Activity,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    Reasoning,
    Command,
    FileChange,
    FileRead,
    FileSearch,
    FileList,
    Search,
    Plan,
    Tool,
    /// The daemon-injected structural workspace map, prepended to a fresh
    /// session's first prompt while the project-map experiment is on.
    ProjectMap,
}

impl ActivityKind {
    /// Classifies provider tool names without mistaking unrelated MCP tools
    /// such as `create_thread` or `read_mcp_resource` for file operations.
    pub fn from_tool_name(name: &str) -> Self {
        let compact = tool_name_leaf(name);

        if matches!(
            compact.as_str(),
            "todo" | "todowrite" | "updateplan" | "plan"
        ) {
            Self::Plan
        } else if matches!(
            compact.as_str(),
            "bash"
                | "command"
                | "execute"
                | "executecommand"
                | "commandexecution"
                | "runcommand"
                | "runterminalcommand"
                | "shell"
                | "shellcommand"
                | "terminal"
        ) {
            Self::Command
        } else if matches!(
            compact.as_str(),
            "applypatch"
                | "create"
                | "createfile"
                | "delete"
                | "deletefile"
                | "edit"
                | "filechange"
                | "fileedit"
                | "editfile"
                | "move"
                | "movefile"
                | "multiedit"
                | "notebookedit"
                | "patch"
                | "rename"
                | "renamefile"
                | "replace"
                | "savefile"
                | "strreplace"
                | "write"
                | "writefile"
        ) {
            Self::FileChange
        } else if matches!(
            compact.as_str(),
            "read" | "fileread" | "readfile" | "readtextfile" | "viewfile"
        ) {
            Self::FileRead
        } else if matches!(
            compact.as_str(),
            "filesearch"
                | "find"
                | "findfiles"
                | "glob"
                | "grep"
                | "ripgrep"
                | "searchfiles"
                | "searchinfiles"
        ) {
            Self::FileSearch
        } else if matches!(
            compact.as_str(),
            "directorylist"
                | "filelist"
                | "list"
                | "listdirectory"
                | "listfiles"
                | "ls"
                | "readdir"
        ) {
            Self::FileList
        } else if matches!(
            compact.as_str(),
            "search" | "searchtool" | "webfetch" | "websearch"
        ) {
            Self::Search
        } else {
            Self::Tool
        }
    }
}

/// The normalized leaf of a provider tool name — lowercased, server/MCP
/// prefixes stripped, separators folded so `goddard_delegate`,
/// `goddard-delegate`, and `mcp__x__goddard_delegate` all compare as
/// `goddarddelegate`.
pub fn tool_name_leaf(name: &str) -> String {
    let normalized = name.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    normalized
        .rsplit("__")
        .next()
        .unwrap_or(&normalized)
        .rsplit([':', '.', '/'])
        .next()
        .unwrap_or(&normalized)
        .replace('_', "")
}

/// Whether a provider tool name dispatches a subagent rather than running
/// inline — `task` (Claude/OpenCode), `subagent` (OpenCode 2), `spawn_agent`
/// (Codex), `goddard_delegate` (Goddard's Pi extension). Exact-leaf match only;
/// an MCP `create_task` does not qualify. `wakudelegate` stays so transcripts
/// recorded before the rename still attribute their tool calls.
pub fn is_delegation_tool_name(name: &str) -> bool {
    matches!(
        tool_name_leaf(name).as_str(),
        "task" | "subagent" | "spawnagent" | "goddarddelegate" | "wakudelegate"
    )
}

/// The daemon's project-map state for one session, streamed while the
/// experiment opt-in is on. Clients surface it as a small status chip and,
/// for `Sent`, a transcript artifact carrying the injected text.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProjectMapStatus {
    /// The workspace index is building.
    Building,
    /// The index is ready — a session's first prompt can be mapped.
    Ready { indexed_files: usize },
    /// A settled turn triggered an incremental refresh; a previously sent
    /// map may lag the code until it finishes.
    Refreshing,
    /// The map was prepended to the session's first prompt. `text` is the
    /// rendered map itself, so clients can show exactly what the provider
    /// received.
    Sent {
        mapped_files: usize,
        indexed_files: usize,
        estimated_tokens: usize,
        text: String,
    },
}

/// Where a sandboxed session's launch is, streamed while the daemon builds
/// the guest's checkpoints and boots the VM. The session's working indicator
/// renders the phase; `Ready` clears it. Ephemeral — setup noise never lands
/// in the persisted transcript.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SandboxSetupStatus {
    /// The VM OS image is absent and downloading — only on a machine's first
    /// sandboxed session.
    DownloadingImage,
    /// A checkpoint layer is being built; `toolchain` names what is
    /// installing (the provider CLI, or the detected runtimes).
    BuildingToolchain { toolchain: String },
    /// The VM is booting from a saved checkpoint.
    BootingVm,
    /// The provider's shared sandbox home holds no credentials and the spec
    /// offers an interactive sign-in — the session waits on it.
    NeedsAuth,
    /// The provider process is up — clients clear the transient status.
    Ready,
}

#[derive(Clone, Debug)]
pub enum DriverEvent {
    /// Client-only acknowledgement that every daemon event through this
    /// sequence has been incorporated into the local session projection.
    /// Providers never emit this and the daemon never serializes it.
    RuntimeEventCursorAdvanced(RuntimeEventCursor),
    Connected {
        provider_cursor: Option<ProviderResumeCursor>,
    },
    /// The provider-owned agent composition this session actually runs. A
    /// fresh Harness session may resolve its deployment default when Goddard did
    /// not name one explicitly, so the driver reports the resolved value.
    AgentPresetSelected(Option<String>),
    /// A provider-owned, automatically generated session title. `None`
    /// clears that fallback but never overwrites a user-owned title.
    AutoTitleUpdated(Option<String>),
    /// The slash commands the live process itself reports — Claude's
    /// stream-json init handshake and ACP's `available_commands_update`.
    /// Authoritative over filesystem discovery, which cannot see plugin or
    /// dynamically registered commands.
    AvailableCommands(Vec<ReportedCommand>),
    /// A client submitted a prompt to this runtime. The daemon publishes it
    /// ahead of the provider's `TurnStarted` so every attached client — not
    /// only the one that typed it — carries the user message and the turn it
    /// opens. The projections those clients save then describe the same
    /// transcript; see [`AgentSession::adopt_submitted_prompt`].
    PromptSubmitted {
        message: String,
        turn_id: Uuid,
        message_id: Uuid,
        /// The task whose agent submitted the prompt through the daemon's
        /// scoped agent commands, or `None` for a human submission.
        sent_by_task: Option<Uuid>,
        /// The prompt is provider-facing only — no client renders a
        /// transcript row for it. Set for the internal "continue" nudge.
        hidden: bool,
    },
    TurnStarted,
    /// The provider's turn ended while detached work it will wake the
    /// session for is still running. The turn stays open — the wake's
    /// `TurnStarted` continues it — and the session shows it is waiting.
    TurnParked,
    TextDelta(String),
    ReasoningDelta(String),
    Activity {
        id: Option<String>,
        kind: ActivityKind,
        title: String,
        detail: Option<String>,
        complete: bool,
    },
    RichActivity(ActivityItem),
    /// Session-level work that can outlive the turn which created it. This is
    /// deliberately separate from transcript activities: completing a turn
    /// must not make a detached process or subagent look complete.
    BackgroundWork(BackgroundWorkEvent),
    Permission {
        request_id: String,
        title: String,
        /// The i18n semantic behind `title`/`detail`, when the daemon
        /// composed them from a known key rather than provider text.
        title_i18n: Option<crate::protocol::WireTranslation>,
        detail: String,
        detail_i18n: Option<crate::protocol::WireTranslation>,
        options: Vec<PermissionOption>,
    },
    /// Structured questions the provider needs answered before it can
    /// continue the active turn. Unlike a permission, this is never
    /// auto-approved: the content itself has to come from the user.
    UserInputRequested {
        request_id: String,
        questions: Vec<UserInputQuestion>,
    },
    ComputerUseUpdated(crate::computer_use::ComputerUseState),
    /// The provider accepted a steering message into the running turn.
    /// `sent_by_task` carries the same provenance as
    /// [`DriverEvent::PromptSubmitted`] when the steer came through the
    /// daemon's scoped agent commands.
    SteerAccepted {
        message: String,
        sent_by_task: Option<Uuid>,
        /// The steer carried daemon-injected context rather than user or
        /// agent text — clients record it on the turn but render no row.
        hidden: bool,
    },
    /// The daemon-owned slice of the session's follow-up queue changed —
    /// an agent prompt was parked, delivered, or cancelled. Carries the
    /// daemon's full snapshot of agent-sourced entries; clients merge it
    /// through [`AgentSession::merge_agent_queued`] so composer-queued
    /// follow-ups are untouched.
    QueuedMessagesChanged {
        messages: Vec<QueuedMessage>,
    },
    /// The provider could not steer the running turn (for example it ended
    /// before the request arrived). The app decides the fallback.
    SteerRejected {
        message: String,
        reason: String,
        /// The i18n semantic behind `reason`, when the daemon composed it
        /// from a known key rather than provider text.
        reason_i18n: Option<crate::protocol::WireTranslation>,
        /// A daemon-injected context steer — the daemon retries delivery on
        /// the next prompt, so clients never surface the rejection.
        hidden: bool,
    },
    /// Context-window occupancy reported by the live stream. Fields arrive at
    /// different moments — token counts with each assistant message, the
    /// window size with the settled turn — so each is optional and the app
    /// merges them into [`ContextUsage`].
    UsageUpdated {
        context_tokens: Option<u64>,
        context_window: Option<u64>,
    },
    /// Account-level rate-limit meters carried by the provider's own stream
    /// (Codex's `account/rateLimits/updated`). Same shape the OAuth fetcher
    /// produces for Claude, so the panel renders both identically.
    PlanUsageUpdated(crate::usage::PlanUsage),
    /// The provider-persisted thread goal changed — set, edited, progressed,
    /// or (`None`) cleared. Carries the whole goal so late subscribers need
    /// no earlier event.
    GoalUpdated(Option<ThreadGoal>),
    /// Project-map state for this session while the experiment is on:
    /// index lifecycle updates plus the `Sent` record of the map that rode
    /// the first prompt.
    ProjectMap(ProjectMapStatus),
    /// Sandbox launch progress for this session, emitted before the
    /// provider process exists. Ephemeral — replayed to nobody, cleared by
    /// `SandboxSetupStatus::Ready` or any setup failure.
    SandboxSetup(SandboxSetupStatus),
    TurnFinished {
        success: bool,
        summary: Option<String>,
        /// The i18n semantic behind `summary`, when the daemon knew it.
        /// Absent on older daemons — clients render `summary` as-is.
        summary_i18n: Option<crate::protocol::WireTranslation>,
    },
    /// A user-facing error whose text is a known i18n key: `message` is the
    /// English fallback, `i18n` lets each client render its own locale.
    /// Provider-supplied error text still travels as [`Self::Error`].
    LocalizedError {
        message: String,
        i18n: crate::protocol::WireTranslation,
    },
    Error(String),
    /// The daemon owning this runtime restarted and the runtime could not
    /// be reattached — its provider process was killed with it. Client-only,
    /// like [`Self::RuntimeEventCursorAdvanced`]: the desktop's daemon proxy
    /// synthesizes it on a failed reattach, so it never crosses the wire.
    RuntimeLost,
    ProcessExited,
}

impl DriverEvent {
    /// Wrap the `(fallback, translation)` pair produced by `localized!`.
    pub fn localized_error(pair: (String, crate::protocol::WireTranslation)) -> Self {
        Self::LocalizedError {
            message: pair.0,
            i18n: pair.1,
        }
    }

    /// Provider-or-fallback error text: carries the i18n semantic when the
    /// daemon composed the message, stays an opaque `Error` when the text
    /// came from the provider.
    pub fn error_or_localized(
        text: String,
        i18n: Option<crate::protocol::WireTranslation>,
    ) -> Self {
        match i18n {
            Some(i18n) => Self::LocalizedError {
                message: text,
                i18n,
            },
            None => Self::Error(text),
        }
    }

    /// A rejected steer whose reason is a known i18n key — the `localized!`
    /// pair supplies both the English fallback and the semantic.
    pub fn steer_rejected_keyed(
        message: String,
        pair: (String, crate::protocol::WireTranslation),
    ) -> Self {
        Self::SteerRejected {
            message,
            reason: pair.0,
            reason_i18n: Some(pair.1),
            hidden: false,
        }
    }

    /// A settled turn whose reason is a known i18n key — the `localized!`
    /// pair supplies both the English fallback and the semantic.
    pub fn turn_finished_keyed(
        success: bool,
        pair: (String, crate::protocol::WireTranslation),
    ) -> Self {
        Self::TurnFinished {
            success,
            summary: Some(pair.0),
            summary_i18n: Some(pair.1),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundWorkKind {
    Process,
    Monitor,
    Subagent,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundWorkStatus {
    Starting,
    Running,
    Monitoring,
    Stopping,
    Completed,
    Failed,
    Stopped,
    Lost,
}

impl BackgroundWorkStatus {
    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Running | Self::Monitoring | Self::Stopping
        )
    }

    pub fn is_stoppable(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Monitoring)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundWorkKey {
    pub kind: BackgroundWorkKind,
    pub provider_id: String,
}

impl BackgroundWorkKey {
    pub fn new(kind: BackgroundWorkKind, provider_id: impl Into<String>) -> Self {
        Self {
            kind,
            provider_id: provider_id.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundWorkItem {
    pub key: BackgroundWorkKey,
    pub title: String,
    /// The i18n semantic behind `title`, when the daemon composed it from a
    /// known key (e.g. the kind's generic label) rather than provider text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_i18n: Option<crate::protocol::WireTranslation>,
    pub detail: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub output: Option<String>,
    pub output_truncated: bool,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    pub duration_ms: Option<u64>,
    pub exit_code: Option<i32>,
    /// Whether the provider considers this detached from the foreground turn.
    pub background: bool,
    pub can_stop: bool,
    /// Provider-native identifier used for an authoritative stop request.
    pub control_id: Option<String>,
    /// Transcript activity that created this work, when the provider exposes it.
    pub origin_activity_id: Option<String>,
    pub role: Option<String>,
    pub model: Option<String>,
    pub parent_id: Option<String>,
    pub status: BackgroundWorkStatus,
}

impl BackgroundWorkItem {
    /// The title to show: the i18n semantic rendered in this process's locale
    /// when present, the daemon's fallback text otherwise.
    pub fn display_title(&self) -> String {
        self.title_i18n
            .as_ref()
            .map(crate::protocol::WireTranslation::render)
            .unwrap_or_else(|| self.title.clone())
    }

    pub fn new(
        kind: BackgroundWorkKind,
        provider_id: impl Into<String>,
        title: impl Into<String>,
        status: BackgroundWorkStatus,
    ) -> Self {
        let now = unix_time_millis();
        Self {
            key: BackgroundWorkKey::new(kind, provider_id),
            title: title.into(),
            title_i18n: None,
            detail: None,
            command: None,
            cwd: None,
            output: None,
            output_truncated: false,
            started_at_ms: now,
            updated_at_ms: now,
            duration_ms: None,
            exit_code: None,
            background: false,
            can_stop: false,
            control_id: None,
            origin_activity_id: None,
            role: None,
            model: None,
            parent_id: None,
            status,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BackgroundWorkEvent {
    Upsert(BackgroundWorkItem),
    OutputDelta {
        key: BackgroundWorkKey,
        delta: String,
    },
    /// Authoritative snapshot of the provider's detached terminal registry.
    ReconcileProcesses {
        items: Vec<BackgroundWorkItem>,
    },
    /// Authoritative snapshot of all provider work still live. Used by
    /// transports which publish a level signal in addition to edge events.
    ReconcileLive {
        items: Vec<BackgroundWorkItem>,
    },
    StopRequested(BackgroundWorkKey),
    StopFailed {
        key: BackgroundWorkKey,
        message: String,
        /// The i18n semantic behind `message`, when the daemon composed it
        /// from a known key rather than provider text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_i18n: Option<crate::protocol::WireTranslation>,
    },
}

/// A slash command a live provider process advertised for its session.
///
/// Claude's init handshake reports bare names; ACP agents report names with
/// descriptions. Sessions persisted by earlier builds stored plain strings,
/// which the untagged repr still accepts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, TS)]
pub struct ReportedCommand {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ReportedCommandRepr {
    Name(String),
    Full {
        name: String,
        #[serde(default)]
        description: String,
    },
}

impl From<ReportedCommandRepr> for ReportedCommand {
    fn from(repr: ReportedCommandRepr) -> Self {
        match repr {
            ReportedCommandRepr::Name(name) => Self {
                name,
                description: String::new(),
            },
            ReportedCommandRepr::Full { name, description } => Self { name, description },
        }
    }
}

impl<'de> Deserialize<'de> for ReportedCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ReportedCommandRepr::deserialize(deserializer).map(Into::into)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub id: String,
    pub label: String,
    /// The i18n semantic behind `label`, when the daemon composed it from a
    /// known key rather than relaying provider text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_i18n: Option<crate::protocol::WireTranslation>,
    pub allow: bool,
}

impl PermissionOption {
    /// A `localized!` pair supplies both the English label and its semantic.
    pub fn keyed(
        id: impl Into<String>,
        pair: (String, crate::protocol::WireTranslation),
        allow: bool,
    ) -> Self {
        Self {
            id: id.into(),
            label: pair.0,
            label_i18n: Some(pair.1),
            allow,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UserInputOption {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UserInputQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    #[serde(default)]
    pub options: Vec<UserInputOption>,
    #[serde(default)]
    pub multi_select: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UserInputAnswer {
    pub question_id: String,
    pub answers: Vec<String>,
}

/// What a provider says happened to a file, when it says anything at all.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ActivityFileChangeStatus {
    Added,
    Modified,
    Deleted,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct ActivityFileChange {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ActivityFileChangeStatus>,
    /// Unified-diff body for this file, normalized once from whatever the
    /// provider sent: a real patch when it supplied one, otherwise synthesized
    /// from its before/after text. Rendering parses this instead of reaching
    /// back into raw tool arguments on every frame.
    ///
    /// Hunk headers are optional here: a bare `@@` line opens a hunk whose
    /// position in the file the provider never told us, which is the common
    /// case for string-replacement edit tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

impl ActivityFileChange {
    pub fn display_name(&self) -> &str {
        Path::new(&self.path)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.path)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct ActivityItem {
    pub id: Uuid,
    #[serde(default)]
    pub source_id: Option<String>,
    pub kind: ActivityKind,
    pub title: String,
    /// The i18n semantic behind `title`, when the daemon composed it from a
    /// known key (an arg-bearing label like "Searching for %{query}") rather
    /// than provider text or a bare kind label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_i18n: Option<crate::protocol::WireTranslation>,
    /// Native tool identity, separate from the human-readable activity title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// MCP server identity, kept separate so clients need not parse tool names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_server: Option<String>,
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Any bounded text field (`output`, `arguments`, `detail`) was clipped
    /// at the source cap. Renderers append the localized truncation marker
    /// at display time; the stored text itself stays locale-neutral.
    #[serde(default, skip_serializing_if = "is_false")]
    pub output_truncated: bool,
    /// Images returned by a tool, kept separate from text so large data URLs
    /// are never truncated or treated as literal activity output.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_urls: Vec<String>,
    #[serde(default)]
    pub failed: bool,
    pub complete: bool,
    /// Provider-neutral edit metadata prepared when the tool event arrives.
    /// Rendering reads this directly instead of reparsing potentially large
    /// patches on every frame.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_changes: Vec<ActivityFileChange>,
    /// Compact subject prepared from native tool input (a file, query,
    /// directory, or command). The row builder only formats this cached value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_target: Option<String>,
    /// Human-authored command description prepared from native tool input.
    /// This stays separate from `display_target` so the UI can prefer a short
    /// label without discarding the raw command used by detail views.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_description: Option<String>,
    /// Native model reasoning carried by the same ordered activity stream as
    /// tool work. Generic provider `think` tools can still use the ordinary
    /// activity fields and leave this empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningBlock>,
}

impl ActivityItem {
    pub fn new(
        source_id: Option<String>,
        kind: ActivityKind,
        title: impl Into<String>,
        detail: Option<String>,
        complete: bool,
    ) -> Self {
        let title = title.into();
        let display_target = fallback_activity_display_target(kind, &title);
        Self {
            id: Uuid::new_v4(),
            source_id,
            kind,
            title,
            title_i18n: None,
            tool_name: None,
            mcp_server: None,
            detail,
            arguments: None,
            output: None,
            output_truncated: false,
            image_urls: Vec::new(),
            failed: false,
            complete,
            file_changes: Vec::new(),
            display_target,
            display_description: None,
            reasoning: None,
        }
    }

    pub fn from_reasoning(reasoning: ReasoningBlock, complete: bool) -> Self {
        Self {
            reasoning: Some(reasoning),
            ..Self::new(None, ActivityKind::Reasoning, "Reasoning", None, complete)
        }
    }

    pub fn with_tool_name(mut self, name: Option<&str>) -> Self {
        if let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) {
            if let Some((server, tool)) = name
                .strip_prefix("mcp__")
                .and_then(|name| name.split_once("__"))
                && !server.is_empty()
                && !tool.is_empty()
            {
                self.mcp_server = Some(server.to_owned());
                self.tool_name = Some(tool.to_owned());
            } else {
                self.tool_name = Some(name.to_owned());
            }
        }
        self
    }

    pub fn with_mcp_server(mut self, server: Option<&str>) -> Self {
        if let Some(server) = server.map(str::trim).filter(|server| !server.is_empty()) {
            self.mcp_server = Some(server.to_owned());
        }
        self
    }

    pub fn with_arguments(mut self, arguments: Option<String>) -> Self {
        self.arguments = arguments;
        self.refresh_activity_metadata();
        self
    }

    pub fn with_activity_source(mut self, source: Option<&serde_json::Value>) -> Self {
        if let Some(source) = source {
            self.refresh_activity_metadata_from_value(source);
        }
        self
    }

    pub fn with_output(mut self, output: Option<String>) -> Self {
        self.output = output;
        self.refresh_command_output();
        self
    }

    pub fn with_image_urls(mut self, image_urls: Vec<String>) -> Self {
        self.image_urls = image_urls;
        self
    }

    pub fn with_failed(mut self, failed: bool) -> Self {
        self.failed = failed;
        self
    }

    /// What this activity says about a session's planning→implementation
    /// boundary. Runs on every streamed event and on rewind re-derivation,
    /// so it stays deterministic and free of evaluation calls: a durable
    /// edit to a non-doc path commits to execution, a shell command or a
    /// doc-shaped write stays ambiguous, and everything else is planning
    /// evidence.
    pub fn phase_signal(&self) -> crate::routing::PhaseSignal {
        use crate::routing::PhaseSignal;
        if self.failed {
            // A refused or failed write is still the agent deciding — not a
            // commitment the turn followed through on.
            return PhaseSignal::Ambiguous;
        }
        match self.kind {
            ActivityKind::FileChange => {
                let doc_only = if self.file_changes.is_empty() {
                    self.display_target.as_deref().is_some_and(plan_doc_shaped)
                } else {
                    self.file_changes
                        .iter()
                        .all(|change| plan_doc_shaped(&change.path))
                };
                if doc_only {
                    PhaseSignal::Ambiguous
                } else {
                    PhaseSignal::Committing
                }
            }
            ActivityKind::Command => PhaseSignal::Ambiguous,
            _ => PhaseSignal::Planning,
        }
    }

    /// Extracts the common tool-input shapes emitted by every provider. This
    /// runs while handling an event (and once for legacy persisted rows), never
    /// from a transcript row builder.
    pub fn refresh_activity_metadata(&mut self) {
        if self.kind != ActivityKind::FileChange {
            self.file_changes.clear();
        }

        let source = self
            .arguments
            .as_deref()
            .map(str::trim)
            .filter(|source| !source.is_empty());
        if let Some(source) = source {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(source) {
                self.refresh_activity_metadata_from_value(&value);
            } else if self.kind == ActivityKind::FileChange {
                let extracted = parse_patch_file_changes(source);
                if !extracted.is_empty() {
                    self.file_changes = extracted;
                }
            }
        }
        if self.kind == ActivityKind::Command {
            self.arguments = self
                .arguments
                .take()
                .and_then(normalize_command_activity_command);
            if let Some(command) = self.arguments.as_deref() {
                self.display_target = Some(compact_activity_target(command));
            }
            self.reclassify_patch_command();
        }
        if self.display_target.is_none() {
            self.display_target = fallback_activity_display_target(self.kind, &self.title);
        }
        self.refresh_command_output();
    }

    /// Refile a shell command that only exists to apply a patch as the file
    /// change it really is. Runs once the command text has been unwrapped from
    /// whatever the provider wrapped it in, so the patch can be read out of it.
    fn reclassify_patch_command(&mut self) {
        let Some(changes) = self
            .arguments
            .as_deref()
            .and_then(apply_patch_command_body)
            .map(parse_patch_file_changes)
            .filter(|changes| !changes.is_empty())
        else {
            return;
        };
        self.kind = ActivityKind::FileChange;
        self.file_changes = changes;
        self.display_target = None;
        self.display_description = None;
    }

    fn refresh_activity_metadata_from_value(&mut self, source: &serde_json::Value) {
        if self.kind == ActivityKind::FileChange {
            let mut extracted = Vec::new();
            extract_file_changes_from_value(source, &mut extracted, 0);
            if !extracted.is_empty() {
                self.file_changes = extracted;
            }
        }
        if let Some(target) = extract_activity_display_target(self.kind, source) {
            self.display_target = Some(target);
        }
        if self.kind == ActivityKind::Command
            && let Some(description) = find_activity_string(source, &["description"], 0)
        {
            self.display_description = Some(compact_activity_target(&description));
        }
    }

    /// Unwrap a tool-result envelope so the run or edit shows what the tool
    /// said rather than the provider's transport JSON around it. Text that is
    /// not a recognized envelope is returned untouched.
    fn refresh_command_output(&mut self) {
        if matches!(self.kind, ActivityKind::Command | ActivityKind::FileChange) {
            self.output = self
                .output
                .take()
                .and_then(normalize_command_activity_output);
        }
    }

    /// The activity's text as an agent-readable excerpt: its title plus the
    /// bounded detail fields, capped at `cap` characters. Reasoning entries
    /// stay private to the provider that produced them and return `None`.
    pub fn condensed_text(&self, cap: usize) -> Option<String> {
        if self.reasoning.is_some() {
            return None;
        }
        let mut text = self.title.clone();
        for field in [
            self.detail.as_deref(),
            self.arguments.as_deref(),
            self.output.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            let field = field.trim();
            if !field.is_empty() {
                text.push_str("\n    ");
                text.push_str(field);
            }
        }
        let text = text.trim();
        if text.is_empty() {
            None
        } else {
            Some(truncate_chars(text, cap))
        }
    }
}

fn normalize_command_activity_command(source: String) -> Option<String> {
    let source = source.trim();
    if source.is_empty() {
        return None;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(source) else {
        return Some(source.to_owned());
    };
    match &value {
        serde_json::Value::String(command) => non_empty_activity_text(command),
        serde_json::Value::Array(parts) if parts.iter().all(|part| part.as_str().is_some()) => {
            non_empty_activity_text(
                &parts
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        }
        serde_json::Value::Object(_) => {
            find_activity_string(&value, &["command", "cmd", "script"], 0)
                .and_then(|command| non_empty_activity_text(&command))
        }
        _ => Some(source.to_owned()),
    }
}

fn normalize_command_activity_output(output: String) -> Option<String> {
    let output = output.trim();
    if output.is_empty() {
        return None;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(output) else {
        return Some(output.to_owned());
    };
    if !is_command_output_envelope(&value) {
        return Some(output.to_owned());
    }
    command_output_envelope_text(&value, 0)
}

fn non_empty_activity_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn is_command_output_envelope(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.contains_key("aggregatedOutput")
        || object.contains_key("structuredContent")
        || object.contains_key("stdout")
        || object.contains_key("stderr")
        || object.contains_key("toolCallId")
        || object.contains_key("tool_call_id")
    {
        return true;
    }
    let output_field = object.contains_key("content")
        || object.contains_key("result")
        || object.contains_key("output");
    if output_field && (object.contains_key("isError") || object.contains_key("is_error")) {
        return true;
    }
    let item_type = object
        .get("type")
        .and_then(serde_json::Value::as_str)
        .map(|value| {
            value
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        });
    if item_type.as_deref().is_some_and(|item_type| {
        matches!(
            item_type,
            "toolresult" | "tooloutput" | "commandresult" | "commandoutput" | "result" | "text"
        )
    }) {
        return true;
    }
    object
        .get("content")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|items| {
            !items.is_empty()
                && items.iter().all(|item| {
                    item.as_object().is_some_and(|item| {
                        item.get("type")
                            .and_then(serde_json::Value::as_str)
                            .is_some()
                    })
                })
        })
}

fn command_output_envelope_text(value: &serde_json::Value, depth: usize) -> Option<String> {
    if depth > 6 {
        return None;
    }
    match value {
        serde_json::Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            if let Ok(nested) = serde_json::from_str::<serde_json::Value>(text)
                && is_command_output_envelope(&nested)
            {
                return command_output_envelope_text(&nested, depth + 1);
            }
            Some(text.to_owned())
        }
        serde_json::Value::Array(items) => {
            let text = items
                .iter()
                .filter(|item| !is_command_output_image(item))
                .filter_map(|item| command_output_content_text(item, depth + 1))
                .collect::<Vec<_>>()
                .join("\n\n");
            non_empty_activity_text(&text)
        }
        serde_json::Value::Object(object) => {
            if object.get("type").and_then(serde_json::Value::as_str) == Some("text") {
                return object
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .and_then(non_empty_activity_text);
            }
            if let Some(structured) = object
                .get("structuredContent")
                .filter(|value| !value.is_null())
            {
                return serde_json::to_string_pretty(structured)
                    .ok()
                    .and_then(|text| non_empty_activity_text(&text));
            }
            if let Some(output) = object
                .get("aggregatedOutput")
                .and_then(serde_json::Value::as_str)
                .and_then(non_empty_activity_text)
            {
                return Some(output);
            }
            let streams = ["stdout", "stderr"]
                .into_iter()
                .filter_map(|key| {
                    object
                        .get(key)
                        .and_then(serde_json::Value::as_str)
                        .and_then(non_empty_activity_text)
                })
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(streams) = non_empty_activity_text(&streams) {
                return Some(streams);
            }
            ["content", "result", "output", "message", "text"]
                .into_iter()
                .find_map(|key| {
                    object
                        .get(key)
                        .filter(|value| !value.is_null())
                        .and_then(|value| command_output_content_text(value, depth + 1))
                })
        }
        serde_json::Value::Null => None,
        value => non_empty_activity_text(&value.to_string()),
    }
}

fn command_output_content_text(value: &serde_json::Value, depth: usize) -> Option<String> {
    if is_command_output_envelope(value) || value.is_array() || value.is_string() {
        return command_output_envelope_text(value, depth);
    }
    if is_command_output_image(value) {
        return None;
    }
    serde_json::to_string_pretty(value)
        .ok()
        .and_then(|text| non_empty_activity_text(&text))
}

fn is_command_output_image(value: &serde_json::Value) -> bool {
    let item_type = value.get("type").and_then(serde_json::Value::as_str);
    let mime = value
        .get("mime")
        .or_else(|| value.get("mimeType"))
        .or_else(|| value.get("mime_type"))
        .and_then(serde_json::Value::as_str);
    matches!(item_type, Some("image" | "inputImage"))
        || (item_type == Some("file") && mime.is_some_and(|mime| mime.starts_with("image/")))
}

/// Whether a file path reads as the plan itself — a doc, spec, or notes
/// file. A write whose only targets are doc-shaped is planning output, not
/// the start of implementation, so it stays ambiguous for the phase signal.
fn plan_doc_shaped(target: &str) -> bool {
    let name = Path::new(target)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(target)
        .to_lowercase();
    if matches!(
        name.rsplit('.').next(),
        Some("md" | "markdown" | "mdx" | "txt" | "rst")
    ) {
        return true;
    }
    let stem = name.split('.').next().unwrap_or_default();
    ["plan", "plans", "spec", "design", "rfc", "proposal"]
        .iter()
        .any(|word| {
            stem.strip_prefix(word).is_some_and(|rest| {
                rest.is_empty() || rest.starts_with('-') || rest.starts_with('_')
            })
        })
}

fn fallback_activity_display_target(kind: ActivityKind, title: &str) -> Option<String> {
    let title = title.trim();
    if title.is_empty() || is_generic_activity_title(kind, title) {
        return None;
    }
    (matches!(kind, ActivityKind::FileRead | ActivityKind::FileList)
        && (title.contains('/') || title.contains('\\') || Path::new(title).extension().is_some()))
    .then(|| compact_activity_target(title))
}

/// The locale every shipped translation renders `key` under. Generic titles
/// persisted by earlier builds were baked in whatever locale the writing
/// process ran, so checking only the current locale misses them.
const SHIPPED_LOCALES: [&str; 3] = ["en", "zh-CN", "ja"];

fn title_in_any_locale(title: &str, keys: &[&str]) -> bool {
    keys.iter().any(|key| {
        SHIPPED_LOCALES
            .iter()
            .any(|locale| title == rust_i18n::t!(*key, locale = *locale))
    })
}

pub fn is_generic_activity_title(kind: ActivityKind, title: &str) -> bool {
    // Classification falling back to `Tool` only means the name is not one of
    // the semantic kinds above. It does not make the provider-supplied tool
    // name generic: otherwise every unknown tool (for example
    // `AskUserQuestion`) loses its identity in the transcript.
    if kind != ActivityKind::Tool && ActivityKind::from_tool_name(title) == kind {
        return true;
    }
    match kind {
        ActivityKind::Command => title_in_any_locale(title, &["activity.run_command"]),
        ActivityKind::FileChange => {
            title_in_any_locale(title, &["activity.edit_file", "activity.write_file"])
        }
        ActivityKind::FileRead => title_in_any_locale(title, &["activity.read_file"]),
        ActivityKind::FileSearch => {
            title_in_any_locale(title, &["activity.search_files", "activity.find_files"])
        }
        ActivityKind::FileList => title_in_any_locale(title, &["activity.list_files"]),
        ActivityKind::Plan => title_in_any_locale(title, &["activity.plan_updated"]),
        ActivityKind::Tool => {
            title.eq_ignore_ascii_case("tool") || title_in_any_locale(title, &["activity.tool"])
        }
        _ => false,
    }
}

fn extract_activity_display_target(
    kind: ActivityKind,
    source: &serde_json::Value,
) -> Option<String> {
    let keys: &[&str] = match kind {
        ActivityKind::Command => &["command", "cmd"],
        ActivityKind::FileRead => &[
            "filePath",
            "file_path",
            "path",
            "targetFile",
            "target_file",
            "notebookPath",
            "notebook_path",
        ],
        ActivityKind::FileSearch => &["pattern", "query", "regex", "glob"],
        ActivityKind::FileList => &["path", "directory", "dir", "root"],
        ActivityKind::Search => &["query", "queries"],
        ActivityKind::Tool => &["title"],
        _ => return None,
    };
    find_activity_string(source, keys, 0).map(|value| compact_activity_target(&value))
}

fn find_activity_string(value: &serde_json::Value, keys: &[&str], depth: usize) -> Option<String> {
    if depth > 4 {
        return None;
    }
    match value {
        serde_json::Value::String(value) => serde_json::from_str::<serde_json::Value>(value)
            .ok()
            .and_then(|nested| find_activity_string(&nested, keys, depth + 1))
            .or_else(|| {
                let value = value.trim();
                (!value.is_empty()).then(|| value.to_owned())
            }),
        serde_json::Value::Array(values) => values
            .iter()
            .find_map(|value| find_activity_string(value, keys, depth + 1)),
        serde_json::Value::Object(object) => {
            for key in keys {
                let Some(value) = object.get(*key) else {
                    continue;
                };
                if let Some(value) = value.as_str().filter(|value| !value.trim().is_empty()) {
                    return Some(value.to_owned());
                }
                if let Some(value) = value.as_array().and_then(|values| {
                    values
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .find(|value| !value.trim().is_empty())
                }) {
                    return Some(value.to_owned());
                }
            }
            for key in [
                "action",
                "arguments",
                "args",
                "input",
                "params",
                "rawInput",
                "raw_input",
                "toolInput",
                "tool_input",
            ] {
                if let Some(value) = object.get(key)
                    && let Some(found) = find_activity_string(value, keys, depth + 1)
                {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn compact_activity_target(value: &str) -> String {
    const MAX_CHARS: usize = 240;
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= MAX_CHARS {
        return compact;
    }
    compact
        .chars()
        .take(MAX_CHARS - 1)
        .chain(std::iter::once('…'))
        .collect()
}

fn extract_file_changes_from_value(
    value: &serde_json::Value,
    changes: &mut Vec<ActivityFileChange>,
    depth: usize,
) {
    if depth > 4 {
        return;
    }
    match value {
        serde_json::Value::String(text) => {
            if let Ok(nested) = serde_json::from_str::<serde_json::Value>(text) {
                extract_file_changes_from_value(&nested, changes, depth + 1);
            } else {
                extend_file_changes(changes, parse_patch_file_changes(text));
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                if let Some(change) = structured_file_change(item, None) {
                    merge_file_change(changes, change);
                } else {
                    extract_file_changes_from_value(item, changes, depth + 1);
                }
            }
        }
        serde_json::Value::Object(object) => {
            for key in ["changes", "fileChanges", "file_changes"] {
                let Some(collection) = object.get(key) else {
                    continue;
                };
                match collection {
                    serde_json::Value::Array(items) => {
                        for item in items {
                            if let Some(change) = structured_file_change(item, None) {
                                merge_file_change(changes, change);
                            } else {
                                extract_file_changes_from_value(item, changes, depth + 1);
                            }
                        }
                    }
                    serde_json::Value::Object(items) => {
                        for (path, item) in items {
                            if let Some(change) = structured_file_change(item, Some(path)) {
                                merge_file_change(changes, change);
                            }
                        }
                    }
                    _ => {}
                }
            }

            for key in ["patch", "patchText", "patch_text"] {
                if let Some(patch) = object.get(key).and_then(serde_json::Value::as_str) {
                    extend_file_changes(changes, parse_patch_file_changes(patch));
                }
            }

            let structured = structured_file_change(value, None);
            if structured.is_none() {
                for key in ["diff", "unifiedDiff", "unified_diff"] {
                    if let Some(patch) = object.get(key).and_then(serde_json::Value::as_str) {
                        extend_file_changes(changes, parse_patch_file_changes(patch));
                    }
                }
            }
            if let Some(change) = structured {
                merge_file_change(changes, change);
            }

            for key in [
                "arguments",
                "args",
                "input",
                "rawInput",
                "raw_input",
                "toolInput",
                "tool_input",
            ] {
                if let Some(nested) = object.get(key) {
                    extract_file_changes_from_value(nested, changes, depth + 1);
                }
            }
        }
        _ => {}
    }
}

fn structured_file_change(
    value: &serde_json::Value,
    fallback_path: Option<&str>,
) -> Option<ActivityFileChange> {
    let object = value.as_object()?;
    let path = [
        "path",
        "filePath",
        "file_path",
        "filename",
        "fileName",
        "targetFile",
        "target_file",
        "notebookPath",
        "notebook_path",
    ]
    .into_iter()
    .find_map(|key| object.get(key).and_then(serde_json::Value::as_str))
    .or(fallback_path)?
    .trim();
    if path.is_empty() {
        return None;
    }

    let change_type = object
        .get("kind")
        .and_then(|kind| {
            kind.as_str()
                .or_else(|| kind.get("type").and_then(serde_json::Value::as_str))
        })
        .or_else(|| object.get("type").and_then(serde_json::Value::as_str));
    let status = match change_type {
        Some("add" | "create" | "added" | "write") => Some(ActivityFileChangeStatus::Added),
        Some("delete" | "deleted" | "remove") => Some(ActivityFileChangeStatus::Deleted),
        Some("update" | "edit" | "modify" | "modified") => Some(ActivityFileChangeStatus::Modified),
        _ => None,
    };

    let patch = ["diff", "unifiedDiff", "unified_diff"]
        .into_iter()
        .find_map(|key| object.get(key).and_then(serde_json::Value::as_str));
    let old = [
        "oldString",
        "old_string",
        "oldStr",
        "old_str",
        "oldText",
        "old_text",
        "oldContent",
        "old_content",
        "oldSource",
        "old_source",
    ]
    .into_iter()
    .find_map(|key| object.get(key).and_then(serde_json::Value::as_str));
    let new = [
        "newString",
        "new_string",
        "newStr",
        "new_str",
        "newText",
        "new_text",
        "newContent",
        "new_content",
        "newSource",
        "new_source",
    ]
    .into_iter()
    .find_map(|key| object.get(key).and_then(serde_json::Value::as_str));
    // A provider that reports an add or a delete puts the whole file where an
    // update would carry a patch, so read that key as content rather than as
    // hunks. Codex's `fileChange` item is the case in the field.
    let whole_file = matches!(
        status,
        Some(ActivityFileChangeStatus::Added | ActivityFileChangeStatus::Deleted)
    )
    .then(|| {
        object
            .get("content")
            .and_then(serde_json::Value::as_str)
            .or(patch)
    })
    .flatten();

    let body = if let Some(hunks) = object
        .get("structuredPatch")
        .or_else(|| object.get("structured_patch"))
        .and_then(serde_json::Value::as_array)
        .filter(|hunks| !hunks.is_empty())
    {
        // The one shape that reports where in the file the change landed.
        // Prefer it over the before/after text beside it, which does not.
        structured_patch_diff(hunks)
    } else if let Some(content) = whole_file {
        whole_file_diff(content, status == Some(ActivityFileChangeStatus::Added))
    } else if let Some(patch) = patch {
        normalize_unified_diff(patch)
    } else if let (Some(old), Some(new)) = (old, new) {
        replacement_diff(old, new)
    } else if let Some(edits) = object.get("edits").and_then(serde_json::Value::as_array) {
        edits_diff(edits)
    } else if let Some(content) = object.get("content").and_then(serde_json::Value::as_str) {
        // A write tool that only names a file and its new contents. Nothing
        // says whether the file already existed, so present it as added.
        whole_file_diff(content, true)
    } else {
        DiffBody::EMPTY
    };

    let (additions, deletions) = match &body.text {
        Some(_) => (Some(body.additions), Some(body.deletions)),
        None => (None, None),
    };
    Some(ActivityFileChange {
        path: path.to_owned(),
        additions,
        deletions,
        status,
        diff: body.text,
    })
}

/// Hunks a provider already positioned in the file, as Claude's edit tools
/// report them alongside their result. Each carries its own start lines, so
/// the rendered diff numbers both sides the way Git would.
fn structured_patch_diff(hunks: &[serde_json::Value]) -> DiffBody {
    let mut text = String::new();
    let mut additions = 0;
    let mut deletions = 0;
    let mut rendered = 0;
    for hunk in hunks {
        let Some(lines) = hunk.get("lines").and_then(serde_json::Value::as_array) else {
            continue;
        };
        let start = |keys: [&str; 2]| {
            keys.into_iter()
                .find_map(|key| hunk.get(key).and_then(serde_json::Value::as_u64))
                .unwrap_or(1)
        };
        let count = |keys: [&str; 2], fallback: usize| {
            keys.into_iter()
                .find_map(|key| hunk.get(key).and_then(serde_json::Value::as_u64))
                .unwrap_or(fallback as u64)
        };
        if rendered < MAX_ACTIVITY_DIFF_LINES {
            text.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                start(["oldStart", "old_start"]),
                count(["oldLines", "old_lines"], lines.len()),
                start(["newStart", "new_start"]),
                count(["newLines", "new_lines"], lines.len()),
            ));
        }
        for line in lines.iter().filter_map(serde_json::Value::as_str) {
            match line.as_bytes().first() {
                Some(b'+') => additions += 1,
                Some(b'-') => deletions += 1,
                _ => {}
            }
            if rendered >= MAX_ACTIVITY_DIFF_LINES {
                continue;
            }
            rendered += 1;
            text.push_str(line);
            text.push('\n');
        }
    }
    DiffBody {
        text: (!text.is_empty()).then_some(text),
        additions,
        deletions,
    }
}

/// One diff over a tool's list of independent replacements, in the order the
/// provider listed them. Each becomes its own hunk.
fn edits_diff(edits: &[serde_json::Value]) -> DiffBody {
    let mut text = String::new();
    let mut additions = 0;
    let mut deletions = 0;
    let mut rendered = 0;
    for edit in edits {
        let Some(edit) = edit.as_object() else {
            continue;
        };
        let old = [
            "oldString",
            "old_string",
            "oldStr",
            "old_str",
            "oldText",
            "old_text",
        ]
        .into_iter()
        .find_map(|key| edit.get(key).and_then(serde_json::Value::as_str));
        let new = [
            "newString",
            "new_string",
            "newStr",
            "new_str",
            "newText",
            "new_text",
        ]
        .into_iter()
        .find_map(|key| edit.get(key).and_then(serde_json::Value::as_str));
        let Some((old, new)) = old.zip(new) else {
            continue;
        };
        let body = replacement_diff(old, new);
        additions += body.additions;
        deletions += body.deletions;
        if let Some(body) = body.text
            && rendered < MAX_ACTIVITY_DIFF_LINES
        {
            rendered += body.lines().count();
            text.push_str(&body);
        }
    }
    DiffBody {
        text: (!text.is_empty()).then_some(text),
        additions,
        deletions,
    }
}

fn parse_patch_file_changes(patch: &str) -> Vec<ActivityFileChange> {
    #[derive(Default)]
    struct PendingChange {
        path: String,
        additions: u64,
        deletions: u64,
        count_lines: bool,
        status: Option<ActivityFileChangeStatus>,
        body: String,
        rendered: usize,
    }

    impl PendingChange {
        /// Keep the hunk in the body being assembled for this file. Counting
        /// continues past the render cap so the badge stays truthful.
        fn keep(&mut self, line: &str) {
            if self.rendered >= MAX_ACTIVITY_DIFF_LINES {
                return;
            }
            self.rendered += 1;
            self.body.push_str(line);
            self.body.push('\n');
        }
    }

    fn finish(pending: &mut Option<PendingChange>, changes: &mut Vec<ActivityFileChange>) {
        let Some(pending) = pending.take() else {
            return;
        };
        if pending.path.is_empty() || pending.path == "/dev/null" {
            return;
        }
        merge_file_change(
            changes,
            ActivityFileChange {
                path: pending.path,
                additions: Some(pending.additions),
                deletions: Some(pending.deletions),
                status: pending.status,
                diff: (!pending.body.is_empty()).then_some(pending.body),
            },
        );
    }

    let mut changes = Vec::new();
    let mut pending: Option<PendingChange> = None;
    for line in patch.lines() {
        let file_marker = [
            ("*** Update File: ", ActivityFileChangeStatus::Modified),
            ("*** Add File: ", ActivityFileChangeStatus::Added),
            ("*** Delete File: ", ActivityFileChangeStatus::Deleted),
        ]
        .into_iter()
        .find_map(|(prefix, status)| line.strip_prefix(prefix).map(|path| (path, status)));
        if let Some((path, status)) = file_marker {
            finish(&mut pending, &mut changes);
            pending = Some(PendingChange {
                path: path.trim().to_owned(),
                count_lines: true,
                status: Some(status),
                ..PendingChange::default()
            });
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Move to: ") {
            if let Some(pending) = pending.as_mut() {
                pending.path = path.trim().to_owned();
            }
            continue;
        }
        if let Some(paths) = line.strip_prefix("diff --git ") {
            finish(&mut pending, &mut changes);
            let path = paths
                .split_whitespace()
                .next_back()
                .map(clean_diff_path)
                .unwrap_or_default();
            pending = Some(PendingChange {
                path,
                ..PendingChange::default()
            });
            continue;
        }
        if line.starts_with("@@") {
            if let Some(pending) = pending.as_mut() {
                pending.count_lines = true;
                pending.keep(line);
            }
            continue;
        }
        if let Some(pending) = pending.as_mut()
            && !pending.count_lines
        {
            if line.starts_with("new file mode ") {
                pending.status = Some(ActivityFileChangeStatus::Added);
            } else if line.starts_with("deleted file mode ") {
                pending.status = Some(ActivityFileChangeStatus::Deleted);
            }
        }
        if pending.as_ref().is_none_or(|pending| !pending.count_lines)
            && let Some(path) = line.strip_prefix("+++ ")
        {
            let path = clean_diff_path(path);
            if path != "/dev/null" {
                if let Some(pending) = pending.as_mut() {
                    pending.path = path;
                } else {
                    pending = Some(PendingChange {
                        path,
                        ..PendingChange::default()
                    });
                }
            }
            continue;
        }
        let Some(pending) = pending.as_mut() else {
            continue;
        };
        if !pending.count_lines {
            continue;
        }
        if line.starts_with('+') {
            pending.additions += 1;
        } else if line.starts_with('-') {
            pending.deletions += 1;
        } else if !line.starts_with(' ') && !line.is_empty() && !line.starts_with('\\') {
            // Codex's dialect ends a file section with the next `*** ` marker
            // and nothing else; anything unmarked here is not diff content.
            continue;
        }
        pending.keep(line);
    }
    finish(&mut pending, &mut changes);
    changes
}

/// The `*** Begin Patch` body of an `apply_patch` run through a shell tool.
///
/// Codex's models edit files by invoking `apply_patch` from the shell rather
/// than through a dedicated tool, and Codex's own TUI recognizes that and
/// presents it as a file change. Doing the same here keeps a real edit from
/// being filed under "ran a command" — and covers any other provider whose
/// model reaches for the same trick.
fn apply_patch_command_body(command: &str) -> Option<&str> {
    const BEGIN: &str = "*** Begin Patch";
    const END: &str = "*** End Patch";

    if !command.contains("apply_patch") {
        return None;
    }
    let start = command.find(BEGIN)?;
    let end = command[start..]
        .find(END)
        .map_or(command.len(), |offset| start + offset + END.len());
    Some(&command[start..end])
}

fn clean_diff_path(path: &str) -> String {
    path.trim()
        .trim_matches('"')
        .strip_prefix("a/")
        .or_else(|| path.trim().trim_matches('"').strip_prefix("b/"))
        .unwrap_or_else(|| path.trim().trim_matches('"'))
        .to_owned()
}

/// Unchanged lines kept around each changed region when a diff is synthesized
/// from a provider's before/after text.
const ACTIVITY_DIFF_CONTEXT_LINES: usize = 3;
/// Ceiling on one stored diff body. Tool arguments are already capped upstream,
/// so this only bounds what synthesis adds — a whole-file write is the case
/// that would otherwise copy an entire source file into the transcript twice.
const MAX_ACTIVITY_DIFF_LINES: usize = 1_000;

/// A synthesized or normalized diff plus the counts taken from the same pass,
/// so the `+N -N` badge can never disagree with the body under it.
struct DiffBody {
    text: Option<String>,
    additions: u64,
    deletions: u64,
}

impl DiffBody {
    const EMPTY: Self = Self {
        text: None,
        additions: 0,
        deletions: 0,
    };
}

/// Diff between the before and after text of a string-replacement edit.
///
/// The fragments carry no position, so hunks open with a bare `@@` rather than
/// invented line numbers. Counts come from the same walk as the body.
fn replacement_diff(old: &str, new: &str) -> DiffBody {
    // Compare over already-split lines: `from_lines` keeps each line's
    // terminator, so a final line without one would never match the same text
    // elsewhere in the file.
    let old = old.lines().collect::<Vec<_>>();
    let new = new.lines().collect::<Vec<_>>();
    let diff = similar::TextDiff::from_slices(&old, &new);
    let mut text = String::new();
    let mut additions = 0;
    let mut deletions = 0;
    let mut rendered = 0;
    for group in diff.grouped_ops(ACTIVITY_DIFF_CONTEXT_LINES) {
        if rendered < MAX_ACTIVITY_DIFF_LINES {
            text.push_str("@@\n");
        }
        for op in &group {
            for change in diff.iter_changes(op) {
                let marker = match change.tag() {
                    similar::ChangeTag::Equal => ' ',
                    similar::ChangeTag::Delete => {
                        deletions += 1;
                        '-'
                    }
                    similar::ChangeTag::Insert => {
                        additions += 1;
                        '+'
                    }
                };
                // Keep counting past the cap: the badge stays truthful even
                // when the body stops.
                if rendered >= MAX_ACTIVITY_DIFF_LINES {
                    continue;
                }
                rendered += 1;
                text.push(marker);
                text.push_str(change.value());
                text.push('\n');
            }
        }
    }
    DiffBody {
        text: (!text.is_empty()).then_some(text),
        additions,
        deletions,
    }
}

/// Diff for a file the provider reported as whole new or whole removed content.
/// One side of the file is empty, so the hunk header carries real positions.
fn whole_file_diff(content: &str, added: bool) -> DiffBody {
    let total = logical_line_count(content);
    if total == 0 {
        return DiffBody::EMPTY;
    }
    let marker = if added { '+' } else { '-' };
    let mut text = if added {
        format!("@@ -0,0 +1,{total} @@\n")
    } else {
        format!("@@ -1,{total} +0,0 @@\n")
    };
    for line in content.lines().take(MAX_ACTIVITY_DIFF_LINES) {
        text.push(marker);
        text.push_str(line);
        text.push('\n');
    }
    DiffBody {
        text: Some(text),
        additions: if added { total } else { 0 },
        deletions: if added { 0 } else { total },
    }
}

/// Strip a provider patch down to hunks. Git file headers only appear before
/// the first hunk, so dropping them by prefix stops there — past it, a line
/// such as `--- x` is a deletion of `-- x`, not a header.
fn normalize_unified_diff(diff: &str) -> DiffBody {
    let mut text = String::with_capacity(diff.len());
    let mut additions = 0;
    let mut deletions = 0;
    let mut rendered = 0;
    // A patch with no hunk header at all is a bare body; open a positionless
    // hunk for it rather than discarding every line looking for a header.
    let mut in_hunk = !diff.lines().any(|line| line.starts_with("@@"));
    if in_hunk {
        text.push_str("@@\n");
    }
    for line in diff.lines() {
        if !in_hunk {
            if !line.starts_with("@@") {
                continue;
            }
            in_hunk = true;
        } else if line.starts_with('+') {
            additions += 1;
        } else if line.starts_with('-') {
            deletions += 1;
        }
        if rendered >= MAX_ACTIVITY_DIFF_LINES {
            continue;
        }
        rendered += 1;
        text.push_str(line);
        text.push('\n');
    }
    DiffBody {
        text: (!text.is_empty()).then_some(text),
        additions,
        deletions,
    }
}

fn logical_line_count(text: &str) -> u64 {
    if text.is_empty() {
        0
    } else {
        text.lines().count() as u64
    }
}

fn extend_file_changes(
    changes: &mut Vec<ActivityFileChange>,
    extracted: impl IntoIterator<Item = ActivityFileChange>,
) {
    for change in extracted {
        merge_file_change(changes, change);
    }
}

fn merge_file_change(changes: &mut Vec<ActivityFileChange>, change: ActivityFileChange) {
    let Some(existing) = changes.iter_mut().find(|item| item.path == change.path) else {
        changes.push(change);
        return;
    };
    match (existing.additions, change.additions) {
        (Some(existing_count), Some(change_count)) => {
            existing.additions = Some(existing_count + change_count);
        }
        (None, Some(change_count)) => existing.additions = Some(change_count),
        _ => {}
    }
    match (existing.deletions, change.deletions) {
        (Some(existing_count), Some(change_count)) => {
            existing.deletions = Some(existing_count + change_count);
        }
        (None, Some(change_count)) => existing.deletions = Some(change_count),
        _ => {}
    }
    existing.status = existing.status.or(change.status);
    match (existing.diff.as_mut(), change.diff) {
        (Some(existing_diff), Some(diff)) => existing_diff.push_str(&diff),
        (None, diff @ Some(_)) => existing.diff = diff,
        _ => {}
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct ReasoningBlock {
    pub content: String,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
}

#[derive(Clone, Debug, TS)]
pub struct TranscriptBlock {
    /// Render this block immediately after this many persisted messages.
    pub after_message: usize,
    pub turn_id: Option<Uuid>,
    /// Ordered non-message work emitted at this point in the transcript.
    /// The persisted field keeps its historical tagged shape so existing
    /// sessions remain readable while the runtime model stays activity-only.
    #[ts(rename = "content", as = "StoredTranscriptBlockContent")]
    pub activities: Vec<ActivityItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "data")]
enum StoredTranscriptBlockContentRef<'a> {
    Activities(&'a [ActivityItem]),
}

#[derive(Deserialize, TS)]
#[serde(rename_all = "camelCase", tag = "kind", content = "data")]
pub enum StoredTranscriptBlockContent {
    Reasoning(ReasoningBlock),
    Activities(Vec<ActivityItem>),
}

#[derive(Serialize)]
struct TranscriptBlockRef<'a> {
    after_message: usize,
    turn_id: Option<Uuid>,
    content: StoredTranscriptBlockContentRef<'a>,
}

impl Serialize for TranscriptBlock {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        TranscriptBlockRef {
            after_message: self.after_message,
            turn_id: self.turn_id,
            content: StoredTranscriptBlockContentRef::Activities(&self.activities),
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
struct TranscriptBlockRepr {
    after_message: usize,
    #[serde(default)]
    turn_id: Option<Uuid>,
    content: StoredTranscriptBlockContent,
}

impl<'de> Deserialize<'de> for TranscriptBlock {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let repr = TranscriptBlockRepr::deserialize(deserializer)?;
        let activities = match repr.content {
            StoredTranscriptBlockContent::Reasoning(reasoning) => {
                vec![ActivityItem::from_reasoning(reasoning, true)]
            }
            StoredTranscriptBlockContent::Activities(activities) => activities,
        };
        Ok(Self {
            after_message: repr.after_message,
            turn_id: repr.turn_id,
            activities,
        })
    }
}

#[derive(Clone, Debug)]
pub struct PendingPermission {
    pub request_id: String,
    pub title: String,
    /// The i18n semantic behind `title`/`detail`, when the daemon composed
    /// them from a known key rather than provider text.
    pub title_i18n: Option<crate::protocol::WireTranslation>,
    pub detail: String,
    pub detail_i18n: Option<crate::protocol::WireTranslation>,
    pub options: Vec<PermissionOption>,
}

pub fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

pub fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

pub fn compact_path(path: &Path) -> String {
    let components = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    if components.len() <= 3 {
        return path.display().to_string();
    }
    format!(
        "…/{}/{}",
        components[components.len() - 2],
        components[components.len() - 1]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_plan_access_mode_loads_as_supervised() {
        let mode: RuntimeMode = serde_json::from_str(r#""plan""#).unwrap();

        assert_eq!(mode, RuntimeMode::Ask);
        assert_eq!(serde_json::to_string(&mode).unwrap(), r#""ask""#);
    }

    #[test]
    fn project_json_without_starred_defaults_to_false() {
        // Projects persisted before the flag existed carry no `starred` key.
        let project: Project = serde_json::from_str(&format!(
            r#"{{"id": "{}", "name": "waku", "path": "/tmp/waku"}}"#,
            Uuid::new_v4()
        ))
        .unwrap();

        assert!(!project.starred);
    }

    #[test]
    fn background_work_snapshots_have_serializable_named_items() {
        let item = BackgroundWorkItem::new(
            BackgroundWorkKind::Process,
            "process-1",
            "server",
            BackgroundWorkStatus::Running,
        );
        let json =
            serde_json::to_value(BackgroundWorkEvent::ReconcileProcesses { items: vec![item] })
                .unwrap();

        assert_eq!(json["type"], "reconcileProcesses");
        assert_eq!(json["items"][0]["key"]["providerId"], "process-1");
        let BackgroundWorkEvent::ReconcileProcesses { items } =
            serde_json::from_value(json).unwrap()
        else {
            panic!("unexpected background-work event");
        };
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn attachment_messages_keep_transport_and_visible_content_separate() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        let attachment = MessageAttachment {
            path: PathBuf::from("/tmp/reference.png"),
            mention: "/tmp/reference.png".to_owned(),
            name: "reference.png".to_owned(),
            is_dir: false,
            is_image: true,
            blob_reference: Some("waku-blob:ab/reference.png".to_owned()),
            pasted_text_preview: None,
            session_id: None,
        };

        session.begin_turn_with_presentation(
            "compare this @/tmp/reference.png",
            Some("compare this".to_owned()),
            vec![attachment.clone()],
        );

        let message = &session.messages[0];
        assert_eq!(message.content, "compare this @/tmp/reference.png");
        assert_eq!(message.visible_content(), "compare this");
        assert_eq!(message.attachments, vec![attachment]);
    }

    #[test]
    fn activity_tool_identity_preserves_names_and_separates_mcp_servers() {
        let mcp = ActivityItem::new(None, ActivityKind::Tool, "Read notes", None, true)
            .with_tool_name(Some("mcp__filesystem__read_file"));
        assert_eq!(mcp.title, "Read notes");
        assert_eq!(mcp.tool_name.as_deref(), Some("read_file"));
        assert_eq!(mcp.mcp_server.as_deref(), Some("filesystem"));
        let regular = ActivityItem::new(None, ActivityKind::Tool, "Read notes", None, true)
            .with_tool_name(Some("read_file"));
        assert_eq!(regular.tool_name.as_deref(), Some("read_file"));
        assert_eq!(regular.mcp_server, None);
        let legacy: ActivityItem = serde_json::from_value(serde_json::json!({
            "id": Uuid::nil(), "kind": "tool", "title": "Read notes", "detail": null,
            "complete": true,
        }))
        .unwrap();
        assert!(legacy.tool_name.is_none());
        assert!(legacy.mcp_server.is_none());
    }

    #[test]
    fn tool_names_are_classified_without_substring_false_positives() {
        for name in [
            "read",
            "ReadFile",
            "read_text_file",
            "mcp__filesystem__read_file",
        ] {
            assert_eq!(
                ActivityKind::from_tool_name(name),
                ActivityKind::FileRead,
                "{name}"
            );
        }
        for name in ["grep", "Glob", "fileSearch", "search_files"] {
            assert_eq!(
                ActivityKind::from_tool_name(name),
                ActivityKind::FileSearch,
                "{name}"
            );
        }
        for name in ["ls", "ListDirectory", "read_dir"] {
            assert_eq!(
                ActivityKind::from_tool_name(name),
                ActivityKind::FileList,
                "{name}"
            );
        }
        for name in ["WriteFile", "applyPatch", "move_file", "str_replace"] {
            assert_eq!(
                ActivityKind::from_tool_name(name),
                ActivityKind::FileChange,
                "{name}"
            );
        }
        for name in ["create_thread", "read_mcp_resource", "list_threads"] {
            assert_eq!(
                ActivityKind::from_tool_name(name),
                ActivityKind::Tool,
                "{name}"
            );
        }
    }

    #[test]
    fn fallback_tool_names_are_not_treated_as_generic_titles() {
        assert!(!is_generic_activity_title(
            ActivityKind::Tool,
            "AskUserQuestion"
        ));
        assert!(!is_generic_activity_title(
            ActivityKind::Tool,
            "mcp__threads__create_thread"
        ));
        assert!(is_generic_activity_title(ActivityKind::Tool, "Tool"));
    }

    #[test]
    fn activity_targets_are_normalized_when_events_arrive() {
        let cases = [
            (
                ActivityKind::FileRead,
                serde_json::json!({"input": {"file_path": "/tmp/waku/src/app.rs"}}),
                "/tmp/waku/src/app.rs",
            ),
            (
                ActivityKind::FileSearch,
                serde_json::json!({"tool_input": {"regex": "ActivityItem"}}),
                "ActivityItem",
            ),
            (
                ActivityKind::FileList,
                serde_json::json!({"arguments": {"directory": "/tmp/waku/src"}}),
                "/tmp/waku/src",
            ),
            (
                ActivityKind::Command,
                serde_json::json!({"args": {"command": "cargo test activity"}}),
                "cargo test activity",
            ),
            (
                ActivityKind::Search,
                serde_json::json!({"action": {"queries": ["Goddard GPUI"]}}),
                "Goddard GPUI",
            ),
            (
                ActivityKind::FileRead,
                serde_json::json!("/tmp/waku/README.md"),
                "/tmp/waku/README.md",
            ),
        ];

        for (kind, arguments, expected) in cases {
            let activity = ActivityItem::new(None, kind, "tool", None, false)
                .with_arguments(Some(arguments.to_string()));
            assert_eq!(
                activity.display_target.as_deref(),
                Some(expected),
                "{kind:?}"
            );
        }

        let described = ActivityItem::new(None, ActivityKind::Command, "bash", None, false)
            .with_arguments(Some(
                serde_json::json!({
                    "command": "python3 analyze.py",
                    "description": "Analyze color statistics"
                })
                .to_string(),
            ));
        assert_eq!(
            described.display_description.as_deref(),
            Some("Analyze color statistics")
        );
        assert_eq!(described.arguments.as_deref(), Some("python3 analyze.py"));
    }

    #[test]
    fn command_output_unwraps_provider_results_without_rewriting_real_output() {
        let wrapped = serde_json::json!({
            "type": "tool-result",
            "toolCallId": "call-1",
            "content": [{
                "type": "text",
                "text": "first line\n{\"actual\":\"command json\"}"
            }],
            "isError": false
        })
        .to_string();
        let activity = ActivityItem::new(None, ActivityKind::Command, "bash", None, true)
            .with_arguments(Some(
                serde_json::json!({
                    "command": "printf output",
                    "description": "Print output"
                })
                .to_string(),
            ))
            .with_output(Some(wrapped));

        assert_eq!(activity.arguments.as_deref(), Some("printf output"));
        assert_eq!(
            activity.output.as_deref(),
            Some("first line\n{\"actual\":\"command json\"}")
        );

        let json_output = r#"{"content":"this came from the command"}"#;
        let activity = ActivityItem::new(None, ActivityKind::Command, "bash", None, true)
            .with_output(Some(json_output.to_owned()));
        assert_eq!(activity.output.as_deref(), Some(json_output));
    }

    #[test]
    fn legacy_command_metadata_is_normalized_on_refresh() {
        let mut activity = ActivityItem::new(None, ActivityKind::Command, "bash", None, true);
        activity.arguments =
            Some(r#"{"command":"git status","description":"Check status"}"#.into());
        activity.output = Some(
            r#"{"type":"tool-result","content":[{"type":"text","text":"clean"}],"isError":false}"#
                .into(),
        );

        activity.refresh_activity_metadata();

        assert_eq!(activity.arguments.as_deref(), Some("git status"));
        assert_eq!(activity.display_target.as_deref(), Some("git status"));
        assert_eq!(
            activity.display_description.as_deref(),
            Some("Check status")
        );
        assert_eq!(activity.output.as_deref(), Some("clean"));
    }

    #[test]
    fn file_edit_metadata_is_normalized_for_every_provider_shape() {
        let cases = [
            (
                ProviderKind::Codex,
                serde_json::json!([{
                    "path": "src/codex.rs",
                    "diff": "@@ -1 +1,2 @@\n-old\n+new\n+next",
                    "kind": {"type": "update"}
                }]),
                "src/codex.rs",
                2,
                1,
            ),
            (
                // A line kept across the replacement is context, not one
                // deletion plus one addition.
                ProviderKind::Claude,
                serde_json::json!({
                    "file_path": "src/claude.rs",
                    "old_string": "old\nline",
                    "new_string": "new\nline\nadded"
                }),
                "src/claude.rs",
                2,
                1,
            ),
            (
                ProviderKind::Amp,
                serde_json::json!({
                    "path": "src/amp.rs",
                    "old_str": "old",
                    "new_str": "new"
                }),
                "src/amp.rs",
                1,
                1,
            ),
            (
                ProviderKind::Cursor,
                serde_json::json!({
                    "input": {
                        "path": "src/cursor.rs",
                        "oldText": "old",
                        "newText": "new\nmore"
                    }
                }),
                "src/cursor.rs",
                2,
                1,
            ),
            (
                ProviderKind::DeepSeek,
                serde_json::json!({
                    "path": "src/deepseek.rs",
                    "oldText": "old",
                    "newText": "new\nmore"
                }),
                "src/deepseek.rs",
                2,
                1,
            ),
            (
                ProviderKind::OpenCode,
                serde_json::json!({
                    "filePath": "src/opencode.rs",
                    "oldString": "same\nold\nend",
                    "newString": "same\nnew\nend"
                }),
                "src/opencode.rs",
                1,
                1,
            ),
            (
                ProviderKind::Grok,
                serde_json::json!({
                    "tool_input": {
                        "patchText": "*** Begin Patch\n*** Update File: src/grok.rs\n@@\n-old\n+new\n+more\n*** End Patch"
                    }
                }),
                "src/grok.rs",
                2,
                1,
            ),
            (
                ProviderKind::Pi,
                serde_json::json!({
                    "path": "src/pi.rs",
                    "edits": [{"oldText": "old", "newText": "new\nmore"}]
                }),
                "src/pi.rs",
                2,
                1,
            ),
        ];

        for (provider, arguments, path, additions, deletions) in cases {
            let activity = ActivityItem::new(
                Some(format!("{}-edit", provider.id())),
                ActivityKind::FileChange,
                "edit",
                None,
                false,
            )
            .with_arguments(Some(arguments.to_string()));
            assert_eq!(activity.file_changes.len(), 1, "{provider:?}");
            let change = &activity.file_changes[0];
            assert_eq!(change.path, path, "{provider:?}");
            assert_eq!(change.additions, Some(additions), "{provider:?}");
            assert_eq!(change.deletions, Some(deletions), "{provider:?}");
            // Every shape must reach rendering as a diff, and the counts the
            // row badge shows must be the ones its body accounts for.
            let diff = change.diff.as_deref().unwrap_or_else(|| {
                panic!("{provider:?} edit should carry a diff body");
            });
            let (rendered_additions, rendered_deletions) = diff
                .lines()
                .skip_while(|line| !line.starts_with("@@"))
                .fold((0, 0), |(added, deleted), line| match line.as_bytes() {
                    [b'+', ..] => (added + 1, deleted),
                    [b'-', ..] => (added, deleted + 1),
                    _ => (added, deleted),
                });
            assert_eq!(rendered_additions, additions, "{provider:?}");
            assert_eq!(rendered_deletions, deletions, "{provider:?}");
        }
    }

    #[test]
    fn positioned_hunks_beat_the_before_and_after_text_beside_them() {
        // Claude's edit tools answer with the patch they applied. Its hunks
        // know where they landed; `old_string`/`new_string` never do.
        let activity = ActivityItem::new(
            Some("toolu_1".into()),
            ActivityKind::FileChange,
            "Edit",
            None,
            true,
        )
        .with_activity_source(Some(&serde_json::json!({
            "filePath": "/tmp/f.txt",
            "oldString": "bravo",
            "newString": "BRAVO",
            "structuredPatch": [{
                "oldStart": 1,
                "oldLines": 4,
                "newStart": 1,
                "newLines": 4,
                "lines": [" alpha", "-bravo", "+BRAVO", " charlie"]
            }]
        })));

        let change = &activity.file_changes[0];
        assert_eq!(change.path, "/tmp/f.txt");
        assert_eq!(change.additions, Some(1));
        assert_eq!(change.deletions, Some(1));
        assert_eq!(
            change.diff.as_deref(),
            Some("@@ -1,4 +1,4 @@\n alpha\n-bravo\n+BRAVO\n charlie\n")
        );
    }

    #[test]
    fn whole_file_writes_diff_against_an_empty_file() {
        let activity = ActivityItem::new(
            Some("write-1".into()),
            ActivityKind::FileChange,
            "Write",
            None,
            true,
        )
        .with_arguments(Some(
            serde_json::json!({
                "file_path": "src/new.rs",
                "content": "fn main() {}\n"
            })
            .to_string(),
        ));

        let change = &activity.file_changes[0];
        assert_eq!(change.additions, Some(1));
        assert_eq!(change.deletions, Some(0));
        // Positions are real here: a created file starts at line 1.
        assert_eq!(
            change.diff.as_deref(),
            Some("@@ -0,0 +1,1 @@\n+fn main() {}\n")
        );
    }

    #[test]
    fn codex_add_and_delete_changes_carry_their_whole_file_as_the_diff() {
        for (kind, additions, deletions, marker) in [
            ("add", Some(2), Some(0), '+'),
            ("delete", Some(0), Some(2), '-'),
        ] {
            let activity = ActivityItem::new(
                Some(format!("codex-{kind}")),
                ActivityKind::FileChange,
                "File Change",
                None,
                true,
            )
            .with_arguments(Some(
                serde_json::json!([{
                    "path": "src/codex.rs",
                    "kind": {"type": kind},
                    "diff": "first\nsecond"
                }])
                .to_string(),
            ));

            let change = &activity.file_changes[0];
            assert_eq!(change.additions, additions, "{kind}");
            assert_eq!(change.deletions, deletions, "{kind}");
            let diff = change.diff.as_deref().expect("whole-file diff");
            assert!(
                diff.lines()
                    .skip(1)
                    .all(|line| line.starts_with(marker) && line.len() > 1),
                "{kind}: {diff}"
            );
        }
    }

    #[test]
    fn apply_patch_run_through_a_shell_becomes_a_file_change() {
        let mut activity = ActivityItem::new(
            Some("exec-1".into()),
            ActivityKind::Command,
            "Shell",
            None,
            true,
        );
        activity.arguments = Some(
            serde_json::json!({
                "command": "apply_patch <<'PATCH'\n*** Begin Patch\n*** Update File: src/one.rs\n@@\n-old\n+new\n*** End Patch\nPATCH",
            })
            .to_string(),
        );
        activity.refresh_activity_metadata();

        assert_eq!(activity.kind, ActivityKind::FileChange);
        assert_eq!(activity.file_changes.len(), 1);
        assert_eq!(activity.file_changes[0].path, "src/one.rs");
        assert_eq!(activity.file_changes[0].additions, Some(1));
        assert_eq!(activity.file_changes[0].deletions, Some(1));
        assert_eq!(
            activity.file_changes[0].diff.as_deref(),
            Some("@@\n-old\n+new\n")
        );
    }

    #[test]
    fn a_command_that_only_mentions_a_patch_stays_a_command() {
        let mut activity = ActivityItem::new(
            Some("exec-2".into()),
            ActivityKind::Command,
            "Shell",
            None,
            true,
        );
        activity.arguments =
            Some(serde_json::json!({"command": "git apply_patch --help"}).to_string());
        activity.refresh_activity_metadata();

        assert_eq!(activity.kind, ActivityKind::Command);
        assert!(activity.file_changes.is_empty());
    }

    #[test]
    fn apply_patch_metadata_keeps_each_file_and_its_counts() {
        let activity = ActivityItem::new(
            Some("patch-1".into()),
            ActivityKind::FileChange,
            "apply_patch",
            None,
            true,
        )
        .with_arguments(Some(
            serde_json::json!({
                "patch": "*** Begin Patch\n*** Update File: src/one.rs\n@@\n-old\n+new\n*** Add File: src/two.rs\n+first\n+second\n*** End Patch"
            })
            .to_string(),
        ));

        assert_eq!(activity.file_changes.len(), 2);
        assert_eq!(activity.file_changes[0].path, "src/one.rs");
        assert_eq!(activity.file_changes[0].additions, Some(1));
        assert_eq!(activity.file_changes[0].deletions, Some(1));
        assert_eq!(activity.file_changes[1].path, "src/two.rs");
        assert_eq!(activity.file_changes[1].additions, Some(2));
        assert_eq!(activity.file_changes[1].deletions, Some(0));
    }

    #[test]
    fn projectless_projects_use_projects_root_and_recognize_legacy_paths() {
        let home = dirs::home_dir().expect("test user has a home directory");
        let root = home.join(crate::identity::HOME_DIRECTORY_NAME);
        let legacy = Project::from_path(root.clone());
        let legacy_dated = Project::from_path(root.join("2026-08-08/new-chat"));
        let project = Project::from_path(root.join("projects/2026-08-08/new-chat"));
        let ordinary = Project::from_path(home.join("dev/goddard"));

        assert!(legacy.is_projectless());
        assert!(legacy_dated.is_projectless());
        assert!(project.is_projectless());
        assert!(!ordinary.is_projectless());
    }

    #[test]
    fn prompt_generates_a_short_session_title() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        session.set_title_from_prompt("build a really polished local agent interface for rust");
        assert_eq!(
            session.auto_title.as_deref(),
            Some("build a really polished local agent interface")
        );
        assert_eq!(
            session.display_title(),
            "build a really polished local agent interface"
        );
        assert_eq!(session.title, AgentSession::DEFAULT_TITLE);
    }

    #[test]
    fn injected_context_blocks_never_reach_a_session_title() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Kimi);
        let injected = "<project-map>\nA structural map.\n</project-map>\n\n\
                        <project-memory>\nDistilled notes.\n</project-memory>\n\n\
                        <goddard-agent>\nA CLI contract.\n</goddard-agent>\n\n\
                        strip the blocks from session titles";
        session.set_title_from_prompt(injected);
        assert_eq!(
            session.auto_title.as_deref(),
            Some("strip the blocks from session titles")
        );

        // A provider that echoes the prompt back as its title — or stores a
        // truncation of it — gets the same treatment on the way in.
        session.set_auto_title(Some(injected.into()));
        assert_eq!(
            session.auto_title.as_deref(),
            Some("strip the blocks from session titles")
        );
        assert!(session.set_auto_title(Some(
            "<project-memory>\nThis project has persistent me".into()
        )));
        assert_eq!(session.auto_title, None);

        // A title that merely mentions the tag is real text and survives.
        assert!(session.set_auto_title(Some("Strip <project-memory> blocks from titles".into())));
        assert_eq!(
            session.auto_title.as_deref(),
            Some("Strip <project-memory> blocks from titles")
        );
    }

    #[test]
    fn provider_title_replaces_prompt_fallback_but_not_an_explicit_title() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::OpenCode);
        session.set_title_from_prompt("investigate the broken provider event");

        assert!(session.set_auto_title(Some("Fix provider title events".into())));
        assert_eq!(session.display_title(), "Fix provider title events");

        assert!(session.set_title("  My title  "));
        assert!(session.set_auto_title(Some("A newer provider title".into())));
        assert_eq!(session.display_title(), "My title");
        assert!(!session.set_title("   "));
        assert_eq!(session.display_title(), "My title");
    }

    #[test]
    fn workspace_move_notice_fires_once_and_names_both_paths() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        session.workspace = SessionWorkspace::Worktree {
            path: PathBuf::from("/tmp/waku-worktrees/task"),
            name: "task".into(),
            branch: None,
            base_branch: None,
        };
        session.workspace_moved_from = Some(PathBuf::from("/tmp/waku"));

        let notice = session.take_workspace_move_notice().unwrap();
        assert!(notice.contains("/tmp/waku-worktrees/task"));
        assert!(notice.contains("/tmp/waku"));
        assert_eq!(session.workspace_moved_from, None);
        assert_eq!(session.take_workspace_move_notice(), None);
    }

    #[test]
    fn workspace_move_notice_waits_for_a_worktree() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        session.workspace_moved_from = Some(PathBuf::from("/tmp/waku"));

        // Still bound to the local checkout: nothing to announce, and the
        // pending flag survives for a later, valid send.
        assert_eq!(session.take_workspace_move_notice(), None);
        assert_eq!(
            session.workspace_moved_from,
            Some(PathBuf::from("/tmp/waku"))
        );
        assert_eq!(session.take_workspace_move_notice(), None);
    }

    #[test]
    fn model_selection_routes_locked_sessions_through_provider_switch() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);

        assert!(session.can_choose_model(ProviderKind::Claude));

        session.push_message(MessageRole::User, "first turn");
        assert!(session.provider_locked());
        // The pick stays allowed — the app routes it through the switch
        // flow — as long as the transcript is loaded to compact.
        assert!(session.can_choose_model(ProviderKind::Codex));
        assert!(session.can_choose_model(ProviderKind::Claude));

        // A skeleton cannot compact its unloaded transcript.
        session.detail_loaded = false;
        assert!(!session.can_choose_model(ProviderKind::Claude));
        assert!(session.can_choose_model(ProviderKind::Codex));
    }

    #[test]
    fn model_selection_ignores_local_assistant_receipts() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);

        session.begin_provider_turn();
        session.push_message(MessageRole::Assistant, "received file receipt");
        session.finish_active_turn(TurnStatus::Completed);

        assert!(!session.provider_locked());
        assert!(session.can_choose_model(ProviderKind::Claude));

        session.begin_provider_turn();
        session.mark_active_turn_provider_started();
        session.finish_active_turn(TurnStatus::Completed);

        assert!(session.provider_locked());
        // Locked no longer blocks the pick — it routes through the
        // provider-switch flow, which compacts the loaded transcript.
        assert!(session.can_choose_model(ProviderKind::Claude));
    }

    #[test]
    fn model_selection_waits_for_the_active_turn_to_finish() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        session.push_message(MessageRole::User, "first turn");

        for status in [
            SessionStatus::Connecting,
            SessionStatus::Working,
            SessionStatus::Waiting,
        ] {
            session.status = status;
            assert!(!session.can_choose_model(ProviderKind::Codex));
        }

        session.status = SessionStatus::Idle;
        assert!(session.can_choose_model(ProviderKind::Codex));
    }

    #[test]
    fn provider_ids_are_stable() {
        assert_eq!(ProviderKind::Amp.id(), "amp");
        assert_eq!(ProviderKind::Claude.id(), "claude");
        assert_eq!(ProviderKind::Codex.command(), "codex");
        assert_eq!(ProviderKind::Cursor.command(), "cursor-agent");
        assert_eq!(ProviderKind::DeepSeek.command(), "dsh");
        assert_eq!(ProviderKind::Devin.id(), "devin");
        assert_eq!(ProviderKind::Devin.command(), "devin");
        assert_eq!(ProviderKind::Devin.display_name(), "Devin CLI");
        assert_eq!(ProviderKind::Droid.id(), "droid");
        assert_eq!(ProviderKind::Droid.command(), "droid");
        assert_eq!(ProviderKind::Droid.display_name(), "Droid");
        assert_eq!(ProviderKind::Fx.command(), "fx");
        assert_eq!(ProviderKind::OpenCode.command(), "opencode");
        assert_eq!(ProviderKind::OpenCode2.id(), "opencode2");
        assert_eq!(ProviderKind::OpenCode2.command(), "opencode2");
        assert_eq!(ProviderKind::Grok.command(), "grok");
        assert_eq!(ProviderKind::Pi.command(), "pi");
    }

    #[test]
    fn provider_setup_is_complete() {
        for provider in ProviderKind::ALL {
            let setup = provider.setup();
            assert!(!setup.install.is_empty(), "{provider:?} has no install");
            assert!(
                setup.docs_url.starts_with("https://"),
                "{provider:?} docs link"
            );
            assert!(
                setup.sign_in.is_some() || setup.api_key_env.is_some(),
                "{provider:?} offers no way to authenticate"
            );
        }
    }

    #[test]
    fn native_conversation_actions_include_every_provider() {
        for provider in ProviderKind::ALL {
            let supported = !matches!(
                provider,
                ProviderKind::Antigravity
                    | ProviderKind::Devin
                    | ProviderKind::Droid
                    | ProviderKind::Fx
                    | ProviderKind::Goose
                    | ProviderKind::Kimi
            );
            assert_eq!(provider.supports_conversation_fork(), supported);
            assert_eq!(provider.supports_conversation_rollback(), supported);
        }
    }

    #[test]
    fn only_dynamic_provider_catalogs_are_discovered() {
        assert!(ProviderKind::Antigravity.supports_model_discovery());
        assert!(!ProviderKind::Amp.supports_model_discovery());
        assert!(ProviderKind::Claude.supports_model_discovery());
        assert!(ProviderKind::Codex.supports_model_discovery());
        assert!(ProviderKind::Copilot.supports_model_discovery());
        assert!(ProviderKind::Cursor.supports_model_discovery());
        assert!(ProviderKind::DeepSeek.supports_model_discovery());
        assert!(ProviderKind::Devin.supports_model_discovery());
        assert!(ProviderKind::Droid.supports_model_discovery());
        assert!(ProviderKind::Fx.supports_model_discovery());
        assert!(!ProviderKind::Goose.supports_model_discovery());
        assert!(ProviderKind::OpenCode.supports_model_discovery());
        assert!(ProviderKind::OpenCode2.supports_model_discovery());
        assert!(ProviderKind::Grok.supports_model_discovery());
        assert!(ProviderKind::Kimi.supports_model_discovery());
        assert!(ProviderKind::OhMyPi.supports_model_discovery());
        assert!(ProviderKind::Pi.supports_model_discovery());
    }

    #[test]
    fn devin_cursor_round_trips_with_its_wire_tag() {
        let cursor = ProviderResumeCursor::from_session_id(ProviderKind::Devin, "ses_dev".into());
        let json = serde_json::to_string(&cursor).unwrap();
        assert!(json.contains("\"provider\":\"devin\""), "{json}");
        assert!(json.contains("\"sessionId\":\"ses_dev\""), "{json}");
        assert_eq!(cursor.provider(), ProviderKind::Devin);
        assert_eq!(cursor.native_id(), "ses_dev");
        assert_eq!(
            serde_json::to_value(ProviderKind::Devin).unwrap(),
            serde_json::json!("devin")
        );
    }

    #[test]
    fn droid_cursor_round_trips_with_its_wire_tag() {
        let cursor = ProviderResumeCursor::from_session_id(ProviderKind::Droid, "ses_dro".into());
        let json = serde_json::to_string(&cursor).unwrap();
        assert!(json.contains("\"provider\":\"droid\""), "{json}");
        assert!(json.contains("\"sessionId\":\"ses_dro\""), "{json}");
        assert_eq!(cursor.provider(), ProviderKind::Droid);
        assert_eq!(cursor.native_id(), "ses_dro");
        assert_eq!(
            serde_json::to_value(ProviderKind::Droid).unwrap(),
            serde_json::json!("droid")
        );
    }

    #[test]
    fn opencode2_cursor_round_trips_with_its_wire_tag() {
        let cursor =
            ProviderResumeCursor::from_session_id(ProviderKind::OpenCode2, "ses_abc".into());
        let json = serde_json::to_string(&cursor).unwrap();
        assert!(json.contains("\"provider\":\"openCode2\""), "{json}");
        assert!(json.contains("\"sessionId\":\"ses_abc\""), "{json}");
        assert_eq!(cursor.provider(), ProviderKind::OpenCode2);
        assert_eq!(cursor.native_id(), "ses_abc");
        assert_eq!(
            serde_json::to_value(ProviderKind::OpenCode2).unwrap(),
            serde_json::json!("openCode2")
        );
    }

    /// OpenCode 2 must not share v1's cursor variant: every driver asserts
    /// `cursor.provider() == provider` before resuming, and a shared variant
    /// would let a v1 session resume against the v2 server.
    #[test]
    fn opencode_cursors_are_distinct_per_major_version() {
        let v1 = ProviderResumeCursor::from_session_id(ProviderKind::OpenCode, "ses_x".into());
        let v2 = ProviderResumeCursor::from_session_id(ProviderKind::OpenCode2, "ses_x".into());
        assert_ne!(v1.provider(), v2.provider());
        assert_ne!(
            serde_json::to_value(&v1).unwrap(),
            serde_json::to_value(&v2).unwrap()
        );
    }

    #[test]
    fn all_contains_every_provider_kind() {
        assert_eq!(ProviderKind::ALL.len(), 18);
        let ids: std::collections::HashSet<_> =
            ProviderKind::ALL.iter().map(|kind| kind.id()).collect();
        assert_eq!(
            ids.len(),
            ProviderKind::ALL.len(),
            "duplicate ProviderKind::id()"
        );
        let commands: std::collections::HashSet<_> = ProviderKind::ALL
            .iter()
            .map(|kind| kind.command())
            .collect();
        assert_eq!(
            commands.len(),
            ProviderKind::ALL.len(),
            "duplicate ProviderKind::command()"
        );
    }

    #[test]
    fn prompt_title_truncation_is_unicode_safe() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Claude);
        let prompt = "界".repeat(70);
        session.set_title_from_prompt(&prompt);
        let title = session.auto_title.as_deref().unwrap();
        assert_eq!(title.chars().count(), 54);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn a_failed_preparation_unwinds_the_turn_it_eagerly_began() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);

        // A first prompt: the unwind restores the default title because the
        // prompt returns to the composer, but keeps the submission activity.
        session.set_title_from_prompt("Build the thing");
        let turn_id = session.begin_turn("Build the thing");
        let submitted_at = session.last_reply_at;
        session.unwind_unstarted_turn(turn_id);
        assert!(session.turns.is_empty());
        assert!(session.messages.is_empty());
        assert_eq!(session.last_reply_at, submitted_at);
        assert_eq!(session.title, AgentSession::DEFAULT_TITLE);
        assert!(session.auto_title.is_none());

        // A follow-up prompt unwinds only itself.
        let first = session.begin_turn("first");
        session.push_message(MessageRole::Assistant, "done");
        session.finish_active_turn(TurnStatus::Completed);
        session.set_title_from_prompt("first");
        let follow_up = session.begin_turn("second");
        let follow_up_submitted_at = session.last_reply_at;
        session.unwind_unstarted_turn(follow_up);
        assert_eq!(session.turns.len(), 1);
        assert_eq!(session.turns[0].id, first);
        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.last_reply_at, follow_up_submitted_at);

        // A turn the provider has already started never unwinds — losing a
        // live conversation turn would desync the provider transcript.
        let started = session.begin_turn("third");
        session.mark_active_turn_provider_started();
        session.unwind_unstarted_turn(started);
        assert_eq!(session.turns.len(), 2);
        assert_eq!(session.messages.len(), 3);
    }

    /// A settled turn whose prompt never reached the provider still unwinds:
    /// the Continue retry resends the prompt rather than nudging a provider
    /// session that has no context for it. A turn the provider confirmed
    /// stays put whichever way it settled.
    #[test]
    fn an_undelivered_settled_turn_unwinds() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);

        let failed = session.begin_turn("the prompt that never sent");
        session.push_notice_message(
            MessageRole::Assistant,
            "Could not start the agent",
            TranscriptNotice::Status {
                kind: TranscriptNoticeStatus::StartFailed,
            },
        );
        session.finish_active_turn(TurnStatus::Failed);
        session.unwind_unstarted_turn(failed);
        assert!(session.turns.is_empty());
        assert!(session.messages.is_empty());

        let interrupted = session.begin_turn("stopped while connecting");
        session.finish_active_turn(TurnStatus::Interrupted);
        session.unwind_unstarted_turn(interrupted);
        assert!(session.turns.is_empty());

        let confirmed = session.begin_turn("the provider saw this one");
        session.mark_active_turn_provider_started();
        session.finish_active_turn(TurnStatus::Failed);
        session.unwind_unstarted_turn(confirmed);
        assert_eq!(session.turns.len(), 1);
        assert_eq!(session.messages.len(), 1);
    }

    #[test]
    fn turn_truncation_removes_owned_messages_and_blocks() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);

        let first_turn = session.begin_turn("first");
        session.push_message(MessageRole::Assistant, "first answer");
        session.transcript_blocks.push(TranscriptBlock {
            after_message: 2,
            turn_id: Some(first_turn),
            activities: Vec::new(),
        });
        session.finish_active_turn(TurnStatus::Completed);

        let second_turn = session.begin_turn("second");
        session.push_message(MessageRole::Assistant, "second answer");
        session.transcript_blocks.push(TranscriptBlock {
            after_message: 4,
            turn_id: Some(second_turn),
            activities: Vec::new(),
        });
        session.finish_active_turn(TurnStatus::Completed);

        session.truncate_after_turn(1);

        assert_eq!(session.turns.len(), 1);
        assert_eq!(session.turns[0].id, first_turn);
        assert_eq!(session.messages.len(), 2);
        assert!(
            session
                .messages
                .iter()
                .all(|message| message.turn_id == Some(first_turn))
        );
        assert_eq!(session.transcript_blocks.len(), 1);
        assert_eq!(session.transcript_blocks[0].turn_id, Some(first_turn));
        assert_eq!(session.transcript_blocks[0].after_message, 2);
    }

    #[test]
    fn agent_transcript_interleaves_activity_and_tags_turns() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);

        let first = session.begin_turn("first prompt");
        session.transcript_blocks.push(TranscriptBlock {
            after_message: 1,
            turn_id: Some(first),
            activities: vec![
                ActivityItem::new(None, ActivityKind::Command, "Ran ls", None, true)
                    .with_output(Some("src/\nREADME.md".into())),
                ActivityItem::from_reasoning(
                    ReasoningBlock {
                        content: "private chain of thought".into(),
                        started_at_ms: 0,
                        finished_at_ms: 1,
                    },
                    true,
                ),
            ],
        });
        session.push_message(MessageRole::Assistant, "first answer");
        session.finish_active_turn(TurnStatus::Completed);

        session.begin_turn("second prompt");
        session.push_message(MessageRole::Assistant, "second answer");
        session.finish_active_turn(TurnStatus::Completed);
        // A provider-facing nudge is recorded but never readable.
        let mut hidden = Message::new(MessageRole::User, "internal continue");
        hidden.hidden = true;
        session.messages.push(hidden);

        let transcript = session.agent_transcript(None);
        let kinds: Vec<_> = transcript
            .items
            .iter()
            .map(|item| (item.kind, item.role, item.turn))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (
                    AgentTranscriptItemKind::Message,
                    Some(MessageRole::User),
                    Some(1)
                ),
                (AgentTranscriptItemKind::Activity, None, Some(1)),
                (
                    AgentTranscriptItemKind::Message,
                    Some(MessageRole::Assistant),
                    Some(1)
                ),
                (
                    AgentTranscriptItemKind::Message,
                    Some(MessageRole::User),
                    Some(2)
                ),
                (
                    AgentTranscriptItemKind::Message,
                    Some(MessageRole::Assistant),
                    Some(2)
                ),
            ]
        );
        assert!(transcript.items[1].content.contains("Ran ls"));
        assert!(transcript.items[1].content.contains("src/"));
        assert!(
            !transcript
                .items
                .iter()
                .any(|item| item.content.contains("private chain of thought")
                    || item.content.contains("internal continue"))
        );
        assert!(!transcript.truncated);

        let turn_two = session.agent_transcript(Some(2));
        assert_eq!(turn_two.items.len(), 2);
        assert!(turn_two.items.iter().all(|item| item.turn == Some(2)));
    }

    #[test]
    fn agent_transcript_drops_the_oldest_items_past_the_total_cap() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        // Each capped message contributes ~8KB; twenty clear the 128KB cap.
        for index in 0..20 {
            session.begin_turn(format!("prompt {index}"));
            session.push_message(MessageRole::Assistant, "x".repeat(20 * 1024));
            session.finish_active_turn(TurnStatus::Completed);
        }

        let transcript = session.agent_transcript(None);
        assert!(transcript.truncated);
        let size: usize = transcript.items.iter().map(|item| item.content.len()).sum();
        assert!(size <= 128 * 1024);
        // The newest turn survives; the dropped prefix keeps turn tagging.
        assert_eq!(transcript.items.last().and_then(|item| item.turn), Some(20));
        assert!(transcript.items.iter().all(|item| item.turn.is_some()));

        // A per-turn read reaches what the capped listing dropped.
        let first = session.agent_transcript(Some(1));
        assert!(!first.truncated);
        assert_eq!(first.items.len(), 2);
        assert!(first.items[0].content.contains("prompt 0"));
    }

    #[test]
    fn transcript_index_groups_cues_per_turn_and_keeps_user_verbatim() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);

        let first = session.begin_turn("first prompt");
        session.transcript_blocks.push(TranscriptBlock {
            after_message: 1,
            turn_id: Some(first),
            activities: vec![
                ActivityItem::new(None, ActivityKind::Command, "Ran ls", None, true)
                    .with_output(Some("src/\nREADME.md".into())),
            ],
        });
        session.push_message(MessageRole::Assistant, "first answer\nmore detail");
        session.finish_active_turn(TurnStatus::Completed);
        session.begin_turn("second prompt");
        session.push_message(MessageRole::Assistant, "second answer");
        session.finish_active_turn(TurnStatus::Completed);
        // A hidden nudge never reaches the index, and the per-turn cue cap
        // bounds a busy turn's lines.
        let mut hidden = Message::new(MessageRole::User, "internal continue");
        hidden.hidden = true;
        session.messages.push(hidden);
        let second = session.begin_turn("busy prompt");
        session.transcript_blocks.push(TranscriptBlock {
            after_message: session.messages.len(),
            turn_id: Some(second),
            activities: (0..12)
                .map(|index| {
                    ActivityItem::new(
                        None,
                        ActivityKind::Command,
                        format!("step {index}"),
                        None,
                        true,
                    )
                })
                .collect(),
        });
        session.finish_active_turn(TurnStatus::Completed);

        let index = session.transcript_index();
        assert_eq!(index.len(), 3);
        assert_eq!(index[0].0, Some(1));
        assert_eq!(
            index[0].1,
            vec![
                "User: first prompt".to_owned(),
                "— Ran ls".to_owned(),
                "— Assistant: first answer".to_owned(),
            ]
        );
        assert_eq!(
            index[1].1,
            vec![
                "User: second prompt".to_owned(),
                "— Assistant: second answer".to_owned(),
            ]
        );
        assert_eq!(index[2].1.len(), 1 + AGENT_INDEX_CUES_PER_TURN);
        assert!(
            !index
                .iter()
                .any(|(_, lines)| { lines.iter().any(|line| line.contains("internal continue")) })
        );

        // The listing cap does not shrink an index — every turn is named.
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        for turn in 0..20 {
            session.begin_turn(format!("prompt {turn}"));
            session.push_message(MessageRole::Assistant, "x".repeat(20 * 1024));
            session.finish_active_turn(TurnStatus::Completed);
        }
        assert_eq!(session.transcript_index().len(), 20);
    }

    #[test]
    fn response_fork_is_a_distinct_idle_session_through_the_selected_turn() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);

        let first_turn = session.begin_turn("first");
        let first_message = session.push_message(MessageRole::Assistant, "first answer");
        session.finish_active_turn(TurnStatus::Completed);
        session.begin_turn("second");
        session.push_message(MessageRole::Assistant, "second answer");
        session.finish_active_turn(TurnStatus::Completed);

        let fork = session
            .fork_through_turn(
                1,
                ProviderResumeCursor::Codex {
                    thread_id: "forked-thread".into(),
                },
                "New task (2)",
            )
            .unwrap();

        assert_ne!(fork.id, session.id);
        assert_eq!(fork.title, AgentSession::DEFAULT_TITLE);
        assert_eq!(fork.auto_title.as_deref(), Some("New task (2)"));
        assert_eq!(fork.status, SessionStatus::Idle);
        assert_eq!(fork.turns.len(), 1);
        assert_eq!(fork.messages.len(), 2);
        assert_ne!(fork.turns[0].id, first_turn);
        assert_ne!(fork.messages[1].id, first_message);
        assert!(
            fork.messages
                .iter()
                .all(|message| message.turn_id == Some(fork.turns[0].id))
        );
        assert!(matches!(
            fork.provider_cursor,
            Some(ProviderResumeCursor::Codex { ref thread_id }) if thread_id == "forked-thread"
        ));
    }

    #[test]
    fn queued_follow_ups_stay_with_the_source_session_not_the_fork() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);

        session.begin_turn("first");
        session.push_message(MessageRole::Assistant, "first answer");
        session.finish_active_turn(TurnStatus::Completed);
        session
            .queued_messages
            .push(QueuedMessage::new("after you finish, also…"));

        let fork = session
            .fork_through_turn(
                1,
                ProviderResumeCursor::Codex {
                    thread_id: "forked-thread".into(),
                },
                "New task (2)",
            )
            .unwrap();

        assert_eq!(session.queued_messages.len(), 1);
        assert!(fork.queued_messages.is_empty());
    }

    #[test]
    fn follow_up_queue_round_trips_through_serde() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        session
            .queued_messages
            .push(QueuedMessage::new("first follow-up"));
        session
            .queued_messages
            .push(QueuedMessage::new("second follow-up"));

        let value = serde_json::to_value(&session).unwrap();
        assert!(value["queued_messages"].is_array());
        let restored: AgentSession = serde_json::from_value(value).unwrap();
        assert_eq!(restored.queued_messages.len(), 2);
        assert_eq!(restored.queued_messages[0].content, "first follow-up");
        assert_eq!(restored.queued_messages[1].content, "second follow-up");
        assert_ne!(
            restored.queued_messages[0].id,
            restored.queued_messages[1].id
        );

        // Sessions without the field (older state files) deserialize as empty.
        let mut legacy = serde_json::to_value(&session).unwrap();
        legacy.as_object_mut().unwrap().remove("queued_messages");
        let legacy_session: AgentSession = serde_json::from_value(legacy).unwrap();
        assert!(legacy_session.queued_messages.is_empty());
    }

    #[test]
    fn queued_message_source_defaults_to_user_and_marks_agent_entries() {
        // Documents written before `source` existed carry no field; every
        // one of those entries was composer-owned.
        let mut value = serde_json::to_value(QueuedMessage::new("follow-up")).unwrap();
        value.as_object_mut().unwrap().remove("source");
        let restored: QueuedMessage = serde_json::from_value(value).unwrap();
        assert_eq!(restored.source, QueuedMessageSource::User);
        assert!(!restored.is_agent_owned());

        let sender = Uuid::new_v4();
        let agent = QueuedMessage::agent("from an agent", Some(sender));
        assert!(agent.is_agent_owned());
        let value = serde_json::to_value(&agent).unwrap();
        let restored: QueuedMessage = serde_json::from_value(value).unwrap();
        assert_eq!(
            restored.source,
            QueuedMessageSource::Agent {
                sent_by: Some(sender)
            }
        );
    }

    #[test]
    fn agent_queue_merge_replaces_only_the_daemon_owned_slice() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);

        let mut user_entry = QueuedMessage::new("mine");
        user_entry.created_at = 10;
        let mut old_agent = QueuedMessage::agent("parked", None);
        old_agent.created_at = 20;
        session.queued_messages = vec![user_entry.clone(), old_agent];

        let mut new_agent = QueuedMessage::agent("freshly parked", Some(Uuid::new_v4()));
        new_agent.created_at = 30;
        assert!(session.merge_agent_queued(vec![new_agent.clone()]));

        // The user entry survives, the superseded agent entry is gone, and
        // the combined queue stays in submission order.
        assert_eq!(session.queued_messages, vec![user_entry, new_agent]);

        // An identical snapshot is a no-op.
        let snapshot = session.queued_messages.clone();
        assert!(
            !session.merge_agent_queued(
                snapshot
                    .iter()
                    .filter(|queued| queued.is_agent_owned())
                    .cloned()
                    .collect()
            )
        );
        assert_eq!(session.queued_messages, snapshot);
    }

    #[test]
    fn adopting_a_submitted_prompt_drops_its_queued_chip() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        let agent_entry = QueuedMessage::agent("parked prompt", None);
        let queued_id = agent_entry.id;
        session.queued_messages.push(agent_entry);
        session
            .queued_messages
            .push(QueuedMessage::new("user draft"));

        let turn_id = Uuid::new_v4();
        assert!(session.adopt_submitted_prompt("parked prompt", turn_id, queued_id, None, false));

        // Only the matching entry left; the delivered message reuses its id.
        assert_eq!(session.queued_messages.len(), 1);
        assert!(!session.queued_messages[0].is_agent_owned());
        let prompt = session.messages.last().unwrap();
        assert_eq!(prompt.id, queued_id);
        assert_eq!(prompt.content, "parked prompt");
    }

    #[test]
    fn planned_worktree_base_branch_is_optional_and_round_trips() {
        let legacy: SessionWorkspace =
            serde_json::from_value(serde_json::json!({ "kind": "newWorktree" })).unwrap();
        assert_eq!(legacy, SessionWorkspace::NewWorktree { base_branch: None });

        let selected = SessionWorkspace::NewWorktree {
            base_branch: Some("release/next".into()),
        };
        let restored = serde_json::from_value(serde_json::to_value(&selected).unwrap()).unwrap();
        assert_eq!(selected, restored);
    }

    #[test]
    fn busy_statuses_cover_connecting_working_and_waiting() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        for status in [
            SessionStatus::Connecting,
            SessionStatus::Working,
            SessionStatus::Waiting,
        ] {
            session.status = status;
            assert!(session.is_busy());
        }
        for status in [SessionStatus::Idle, SessionStatus::Failed] {
            session.status = status;
            assert!(!session.is_busy());
        }
    }

    #[test]
    fn provider_resume_cursor_is_explicitly_tagged() {
        let cursor = ProviderResumeCursor::Claude {
            session_id: "session-1".into(),
            resume_at: Some("message-9".into()),
        };
        let value = serde_json::to_value(&cursor).unwrap();
        assert_eq!(value["provider"], "claude");
        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["resumeAt"], "message-9");

        let cursor = ProviderResumeCursor::Cursor {
            session_id: String::new(),
            fork_context: Some("[]".into()),
        };
        let value = serde_json::to_value(&cursor).unwrap();
        assert_eq!(value["provider"], "cursor");
        assert_eq!(value["sessionId"], "");
        assert_eq!(value["forkContext"], "[]");
    }

    #[test]
    fn native_rollback_count_ignores_turns_that_never_reached_the_provider() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);

        session.begin_turn("first");
        session.mark_active_turn_provider_started();
        session.finish_active_turn(TurnStatus::Completed);
        session.begin_turn("failed locally");
        session.finish_active_turn(TurnStatus::Failed);
        session.begin_turn("third");
        session.mark_active_turn_provider_started();
        session.finish_active_turn(TurnStatus::Completed);

        assert_eq!(session.provider_turns_after(1), 1);
        assert_eq!(session.provider_turns_after(2), 1);
        assert_eq!(session.provider_turns_after(3), 0);
    }

    #[test]
    fn legacy_empty_search_titles_are_repaired() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        session.transcript_blocks.push(TranscriptBlock {
            after_message: 0,
            turn_id: None,
            activities: vec![ActivityItem::new(
                Some("search-1".into()),
                ActivityKind::Search,
                "Search for ",
                None,
                true,
            )],
        });

        session.migrate_legacy_state();

        let activities = &session.transcript_blocks[0].activities;
        assert_eq!(activities[0].title, "Browsed the web");
    }

    #[test]
    fn legacy_reasoning_blocks_deserialize_as_reasoning_activities() {
        let legacy = serde_json::json!({
            "after_message": 2,
            "turn_id": null,
            "content": {
                "kind": "reasoning",
                "data": {
                    "content": "Checking the source",
                    "started_at_ms": 1_000,
                    "finished_at_ms": 2_500
                }
            }
        });

        let block: TranscriptBlock = serde_json::from_value(legacy).unwrap();
        assert_eq!(block.activities.len(), 1);
        let activity = &block.activities[0];
        assert_eq!(activity.kind, ActivityKind::Reasoning);
        assert!(activity.complete);
        assert_eq!(
            activity
                .reasoning
                .as_ref()
                .map(|reasoning| reasoning.content.as_str()),
            Some("Checking the source")
        );

        let stored = serde_json::to_value(block).unwrap();
        assert_eq!(stored["content"]["kind"], "activities");
        assert_eq!(
            stored["content"]["data"][0]["reasoning"]["content"],
            "Checking the source"
        );
    }

    #[test]
    fn adjacent_legacy_work_blocks_merge_during_session_migration() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        session.transcript_blocks.extend([
            TranscriptBlock {
                after_message: 1,
                turn_id: None,
                activities: vec![ActivityItem::from_reasoning(
                    ReasoningBlock {
                        content: "Looking around".into(),
                        started_at_ms: 1_000,
                        finished_at_ms: 2_000,
                    },
                    true,
                )],
            },
            TranscriptBlock {
                after_message: 1,
                turn_id: None,
                activities: vec![ActivityItem::new(
                    None,
                    ActivityKind::Command,
                    "Ran tests",
                    None,
                    true,
                )],
            },
        ]);

        session.migrate_legacy_state();

        assert_eq!(session.transcript_blocks.len(), 1);
        assert_eq!(session.transcript_blocks[0].activities.len(), 2);
        assert_eq!(
            session.transcript_blocks[0]
                .activities
                .iter()
                .map(|activity| activity.kind)
                .collect::<Vec<_>>(),
            [ActivityKind::Reasoning, ActivityKind::Command]
        );
    }

    #[test]
    fn legacy_file_edit_details_are_promoted_to_arguments_and_metadata() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::OpenCode);
        session.transcript_blocks.push(TranscriptBlock {
            after_message: 0,
            turn_id: None,
            activities: vec![ActivityItem::new(
                None,
                ActivityKind::FileChange,
                "edit",
                Some(
                    serde_json::json!({
                        "filePath": "/tmp/waku/README.md",
                        "oldString": "old",
                        "newString": "new\nmore"
                    })
                    .to_string(),
                ),
                true,
            )],
        });

        session.migrate_legacy_state();

        let activities = &session.transcript_blocks[0].activities;
        assert!(activities[0].detail.is_none());
        assert!(activities[0].arguments.is_some());
        assert_eq!(activities[0].file_changes[0].path, "/tmp/waku/README.md");
        assert_eq!(activities[0].file_changes[0].additions, Some(2));
        assert_eq!(activities[0].file_changes[0].deletions, Some(1));
    }

    #[test]
    fn legacy_file_tools_are_reclassified_and_gain_cached_targets() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::OpenCode);
        let mut cached = ActivityItem::new(None, ActivityKind::FileRead, "read", None, true);
        cached.display_target = Some("/tmp/waku/src/persisted.rs".into());
        session.transcript_blocks.push(TranscriptBlock {
            after_message: 0,
            turn_id: None,
            activities: vec![
                ActivityItem::new(
                    None,
                    ActivityKind::Search,
                    "read",
                    Some(r#"{"filePath":"/tmp/waku/src/model.rs"}"#.into()),
                    true,
                ),
                ActivityItem::new(
                    None,
                    ActivityKind::Tool,
                    "glob",
                    Some(r#"{"pattern":"src/**/*.rs"}"#.into()),
                    true,
                ),
                cached,
            ],
        });

        session.migrate_legacy_state();

        let activities = &session.transcript_blocks[0].activities;
        assert_eq!(activities[0].kind, ActivityKind::FileRead);
        assert_eq!(
            activities[0].display_target.as_deref(),
            Some("/tmp/waku/src/model.rs")
        );
        assert_eq!(activities[1].kind, ActivityKind::FileSearch);
        assert_eq!(activities[1].display_target.as_deref(), Some("src/**/*.rs"));
        assert_eq!(
            activities[2].display_target.as_deref(),
            Some("/tmp/waku/src/persisted.rs")
        );
        assert!(
            activities[..2]
                .iter()
                .all(|activity| activity.detail.is_none())
        );
        assert!(
            activities[..2]
                .iter()
                .all(|activity| activity.arguments.is_some())
        );
    }

    #[test]
    fn legacy_codex_citation_markers_are_removed() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        session.messages.push(Message::new(
            MessageRole::Assistant,
            "Claim.\u{e200}cite\u{e202}turn3view0\u{e202}turn2view2\u{e201}\nNext.",
        ));

        session.migrate_legacy_state();

        assert_eq!(session.messages[0].content, "Claim.\nNext.");
    }

    #[test]
    fn legacy_checkpoint_totals_are_backfilled_from_the_file_summary() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        session.begin_turn("Build it");
        session.finish_active_turn(TurnStatus::Completed);
        let mut serialized = serde_json::to_value(Checkpoint {
            turn_count: 1,
            git_ref: "refs/waku/test".into(),
            status: CheckpointStatus::Ready,
            files: vec![
                CheckpointFile {
                    path: "src/app.rs".into(),
                    additions: 7,
                    deletions: 2,
                },
                CheckpointFile {
                    path: "src/model.rs".into(),
                    additions: 3,
                    deletions: 5,
                },
            ],
            additions: 10,
            deletions: 7,
            created_at: 1,
        })
        .unwrap();
        let object = serialized.as_object_mut().unwrap();
        object.remove("additions");
        object.remove("deletions");
        let checkpoint: Checkpoint = serde_json::from_value(serialized).unwrap();
        assert_eq!((checkpoint.additions, checkpoint.deletions), (0, 0));
        session.turns[0].checkpoint = Some(checkpoint);

        session.migrate_legacy_state();

        let checkpoint = session.turns[0].checkpoint.as_ref().unwrap();
        assert_eq!((checkpoint.additions, checkpoint.deletions), (10, 7));
        assert!(checkpoint.totals_are_current());
    }

    #[test]
    fn a_follower_adopts_another_clients_submission_under_its_ids() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("first");
        session.push_message(MessageRole::Assistant, "done");
        session.finish_active_turn(TurnStatus::Completed);
        session.status = SessionStatus::Idle;
        let turn_id = Uuid::new_v4();
        let message_id = Uuid::new_v4();

        assert!(session.adopt_submitted_prompt("second", turn_id, message_id, None, false));

        assert_eq!(session.status, SessionStatus::Connecting);
        assert_eq!(session.active_turn_id(), Some(turn_id));
        let turn = session.turns.last().unwrap();
        assert_eq!(turn.turn_count, 2);
        assert!(!turn.provider_turn_started);
        let prompt = session.messages.last().unwrap();
        assert_eq!(prompt.id, message_id);
        assert_eq!(prompt.turn_id, Some(turn_id));
        assert_eq!(prompt.role, MessageRole::User);
        assert_eq!(prompt.content, "second");
    }

    #[test]
    fn the_submitters_own_echo_changes_nothing() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        let turn_id = session.begin_turn("first");
        session.status = SessionStatus::Connecting;
        let message_id = session.messages[0].id;

        assert!(!session.adopt_submitted_prompt("first", turn_id, message_id, None, false));

        assert_eq!(session.turns.len(), 1);
        assert_eq!(session.messages.len(), 1);
        assert_eq!(session.status, SessionStatus::Connecting);
    }

    #[test]
    fn a_provider_started_turn_takes_the_submitted_prompt_as_its_own() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Claude);
        let provider_turn = session.begin_provider_turn();
        session.mark_active_turn_provider_started();
        session.status = SessionStatus::Working;
        let message_id = Uuid::new_v4();

        assert!(session.adopt_submitted_prompt(
            "continue",
            Uuid::new_v4(),
            message_id,
            None,
            false
        ));

        assert_eq!(session.turns.len(), 1);
        let prompt = session.messages.last().unwrap();
        assert_eq!(prompt.id, message_id);
        assert_eq!(prompt.turn_id, Some(provider_turn));
        assert_eq!(prompt.role, MessageRole::User);
        assert_eq!(session.status, SessionStatus::Working);
    }

    #[test]
    fn list_projection_never_copies_session_detail() {
        let project = Project::from_path(PathBuf::from("/tmp/waku"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        session.title = "Visible title".into();
        session.model = Some("gpt-5".into());
        session.status = SessionStatus::Working;
        session.workspace = SessionWorkspace::Worktree {
            path: PathBuf::from("/tmp/waku-worktrees/task"),
            name: "task".into(),
            branch: Some("waku/task".into()),
            base_branch: None,
        };
        session.begin_turn("A large prompt");
        session.messages.push(Message::new(
            MessageRole::Assistant,
            "A large streamed response",
        ));
        session.transcript_blocks.push(TranscriptBlock {
            after_message: 1,
            turn_id: None,
            activities: vec![ActivityItem::new(
                None,
                ActivityKind::Tool,
                "Inspect files",
                None,
                false,
            )],
        });
        session
            .queued_messages
            .push(QueuedMessage::new("Follow up"));

        let projection = session.list_projection();

        assert_eq!(projection.id, session.id);
        assert_eq!(projection.title, "Visible title");
        assert_eq!(projection.model.as_deref(), Some("gpt-5"));
        assert_eq!(projection.status, SessionStatus::Working);
        assert_eq!(projection.workspace, session.workspace);
        assert!(!projection.detail_loaded);
        assert!(projection.messages.is_empty());
        assert!(projection.transcript_blocks.is_empty());
        assert!(projection.turns.is_empty());
        assert!(projection.queued_messages.is_empty());
    }
}
