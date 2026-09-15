//! User-owned custom commands shared through daemon settings.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// The icon a custom command shows in the command palette, on its settings
/// row, and on its terminal tab.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CustomCommandIcon {
    #[default]
    Terminal,
    Command,
    Zap,
    Wrench,
    Gauge,
    Package,
    GitBranch,
    GitHub,
    Folder,
    File,
    Search,
    Globe,
    Server,
    CloudUpload,
    Download,
    Bot,
    Sparkle,
    Star,
    Target,
    Queue,
    Compose,
    Chart,
    Refresh,
    Archive,
}

/// A user-owned shell command listed in the command palette. Running one
/// opens an interactive terminal tab that sources a materialized copy of
/// `script` — identical in effect to pasting the text into that shell.
///
/// Commands live in [`crate::settings::DaemonSettings`] rather than in one
/// client's app file: every attached client shares the list, the scoped
/// agent settings surface can write it, and the scripts execute on the
/// daemon host regardless of which client started them.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CustomCommand {
    pub id: Uuid,
    /// Palette label; `None` (or blank) falls back to the script itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Icon the command shows in the palette, settings list, and tab.
    #[serde(default)]
    pub icon: CustomCommandIcon,
    /// Shell the terminal runs; `None` (or blank) uses the platform default
    /// (`$SHELL` on Unix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    pub script: String,
    /// Close the terminal tab automatically once the script exits
    /// successfully; a failure leaves it open for inspection.
    #[serde(default)]
    pub close_on_success: bool,
    /// The Waku task whose agent added or last rewrote this command, when one
    /// did. `None` marks a command the user configured themselves. A client
    /// editing an agent's command keeps the attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_task: Option<Uuid>,
}

impl CustomCommand {
    pub fn new(script: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: None,
            icon: CustomCommandIcon::default(),
            shell: None,
            script,
            close_on_success: false,
            created_by_task: None,
        }
    }

    /// What the palette row and terminal tab call this command.
    pub fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.script)
    }
}

impl CustomCommandIcon {
    pub const ALL: [Self; 24] = [
        Self::Terminal,
        Self::Command,
        Self::Zap,
        Self::Wrench,
        Self::Gauge,
        Self::Package,
        Self::GitBranch,
        Self::GitHub,
        Self::Folder,
        Self::File,
        Self::Search,
        Self::Globe,
        Self::Server,
        Self::CloudUpload,
        Self::Download,
        Self::Bot,
        Self::Sparkle,
        Self::Star,
        Self::Target,
        Self::Queue,
        Self::Compose,
        Self::Chart,
        Self::Refresh,
        Self::Archive,
    ];

    /// Icon names are product names and stay untranslated.
    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Command => "Command",
            Self::Zap => "Zap",
            Self::Wrench => "Wrench",
            Self::Gauge => "Gauge",
            Self::Package => "Package",
            Self::GitBranch => "Git branch",
            Self::GitHub => "GitHub",
            Self::Folder => "Folder",
            Self::File => "File",
            Self::Search => "Search",
            Self::Globe => "Globe",
            Self::Server => "Server",
            Self::CloudUpload => "Upload",
            Self::Download => "Download",
            Self::Bot => "Bot",
            Self::Sparkle => "Sparkle",
            Self::Star => "Star",
            Self::Target => "Target",
            Self::Queue => "Queue",
            Self::Compose => "Compose",
            Self::Chart => "Chart",
            Self::Refresh => "Refresh",
            Self::Archive => "Archive",
        }
    }
}
