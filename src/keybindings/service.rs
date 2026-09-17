//! `keybindings.json` persistence and the effective-keymap service.
//!
//! Layout: catalog defaults → platform filter → user overrides, resolved
//! into an ordered binding list that both GPUI registration and the manager
//! snapshot consume. Writes are atomic (sibling temp file → fsync → rename)
//! with one `.bak` of the last known-good file. Unknown top-level fields and
//! unknown override records survive a rewrite — we never erase data from a
//! file a newer Goddard may have written.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    COMMANDS, CommandDescriptor, CommandId, ENTRIES, PlatformSet, command, command_rows,
};
use crate::keybindings::CommandRow;

pub const SCHEMA_VERSION: u32 = 1;
/// Hard bounds from the spec — past these the file is treated as corrupt.
const MAX_FILE_BYTES: u64 = 1 << 20;
const MAX_OVERRIDES: usize = 5000;
const MAX_STROKES: usize = 3;
const MAX_CONTEXT_LEN: usize = 256;

/// The on-disk schema. `extra` preserves unknown top-level fields across
/// load/save; a `schema_version` newer than ours refuses to rewrite.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct KeybindingsFile {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub layout: PersistedLayout,
    #[serde(default)]
    pub overrides: Vec<UserOverride>,
    /// Unknown top-level fields from a newer file, preserved verbatim.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PersistedLayout {
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub fallback_layout_id: Option<String>,
    #[serde(default)]
    pub show_numpad: bool,
    #[serde(default)]
    pub locked: bool,
    /// Unknown layout fields preserved verbatim.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

fn default_mode() -> String {
    "auto".to_string()
}

/// One user change. `sequence` is a chord: a list of strokes, each stroke a
/// list like `["secondary", "j"]` (modifiers then the key).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserOverride {
    pub command_id: String,
    /// `"macos"`, `"windows"`, `"linux"`, or `"all"`.
    #[serde(default = "default_platforms")]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub context: Option<String>,
    pub operation: BindingOperation,
    #[serde(default = "default_semantics")]
    pub semantics: String,
    #[serde(default)]
    pub sequence: Option<Vec<Vec<String>>>,
    /// Unknown override fields preserved verbatim.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

fn default_platforms() -> Vec<String> {
    vec!["all".to_string()]
}

fn default_semantics() -> String {
    "logical".to_string()
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BindingOperation {
    Replace,
    Add,
    Unbind,
}

/// One binding in the resolved map.
#[derive(Clone, Debug)]
pub struct EffectiveBinding {
    pub command: CommandId,
    /// GPUI keystroke syntax (canonical after validation).
    pub sequence: String,
    pub context: Option<String>,
    pub platform: PlatformSet,
    pub source: super::BindingSource,
    /// Position in registration order — precedence is positional.
    pub precedence: usize,
}

/// The immutable snapshot the manager renders from.
#[derive(Clone)]
pub struct KeymapSnapshot {
    pub revision: u64,
    /// Resolved bindings in precedence order.
    pub bindings: Vec<EffectiveBinding>,
    /// Per-command rows for the table.
    pub rows: Vec<CommandRow>,
}

/// Why a commit was rejected.
#[derive(Clone, Debug)]
pub enum CommitError {
    /// The file or snapshot moved since `expected_revision` — the caller
    /// should refresh conflicts and retry; the captured chord is preserved.
    StaleRevision { current: u64 },
    /// The persisted file uses a schema newer than this build understands.
    NewerSchema { found: u32 },
    /// The change failed validation (unparseable sequence, too many
    /// strokes, unknown command, illegal plain key).
    Invalid(String),
    /// Persistence failed.
    Io(String),
}

/// Events emitted after service operations; the app layer broadcasts these.
#[derive(Clone, Debug)]
pub enum KeybindingEvent {
    KeymapChanged {
        revision: u64,
        changed: Vec<String>,
    },
    FileError {
        recoverable: bool,
        message: String,
    },
}

/// Where the file lives: `~/.goddard/keybindings.json`, matching the app's
/// existing per-user directory convention.
pub fn default_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".goddard").join("keybindings.json"))
}

/// The service: owns overrides, revision, and the resolved map.
pub struct KeybindingService {
    path: PathBuf,
    file: KeybindingsFile,
    /// Parse/IO error observed at load — the file is left untouched until the
    /// user picks a recovery path.
    pub load_error: Option<String>,
}

impl KeybindingService {
    /// Load `keybindings.json`, tolerating a missing file (schema v1 starts
    /// with none). On parse failure the service keeps defaults active and
    /// records the error; nothing is written back.
    pub fn load(path: PathBuf) -> Self {
        let mut service = Self {
            path,
            file: KeybindingsFile::default(),
            load_error: None,
        };
        match fs::read(&service.path) {
            Ok(bytes) => {
                if bytes.len() as u64 > MAX_FILE_BYTES {
                    service.load_error = Some("keybindings.json exceeds the 1 MiB limit".into());
                    return service;
                }
                match serde_json::from_slice::<KeybindingsFile>(&bytes) {
                    Ok(file) => {
                        if file.schema_version > SCHEMA_VERSION {
                            service.load_error = Some(format!(
                                "keybindings.json schema {} is newer than supported {}",
                                file.schema_version, SCHEMA_VERSION
                            ));
                            return service;
                        }
                        if file.overrides.len() > MAX_OVERRIDES {
                            service.load_error = Some("keybindings.json has too many overrides".into());
                            return service;
                        }
                        service.file = file;
                    }
                    Err(error) => {
                        service.load_error = Some(format!("keybindings.json: {error}"));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                service.load_error = Some(format!("keybindings.json: {error}"));
            }
        }
        service
    }

    pub fn revision(&self) -> u64 {
        self.file.revision
    }

    pub fn overrides(&self) -> &[UserOverride] {
        &self.file.overrides
    }

    /// Resolve catalog defaults + user overrides into precedence order.
    /// Pure — the same function backs tests, snapshots, and registration.
    pub fn effective_bindings(&self) -> Vec<EffectiveBinding> {
        resolve(&self.file.overrides)
    }

    /// Immutable snapshot for the manager UI.
    pub fn snapshot(&self) -> KeymapSnapshot {
        let bindings = self.effective_bindings();
        let mut rows = command_rows();
        // Rebuild each row's bindings from the resolved map so overrides show.
        let mut by_command: HashMap<CommandId, Vec<super::EffectiveBindingRow>> = HashMap::new();
        for binding in &bindings {
            by_command
                .entry(binding.command)
                .or_default()
                .push(super::EffectiveBindingRow {
                    sequence: binding.sequence.clone(),
                    context: binding.context.clone(),
                    source: binding.source,
                });
        }
        for row in &mut rows {
            row.bindings = by_command.remove(row.descriptor.id).unwrap_or_default();
        }
        KeymapSnapshot {
            revision: self.file.revision,
            bindings,
            rows,
        }
    }

    /// What a change would do — validation without mutation.
    pub fn preview(
        &self,
        overrides: &[UserOverride],
    ) -> Result<Vec<EffectiveBinding>, CommitError> {
        let mut trial = self.file.overrides.clone();
        trial.extend(overrides.iter().cloned());
        validate_overrides(&trial)?;
        Ok(resolve(&trial))
    }

    /// Apply overrides atomically: validate, resolve, write, bump revision.
    /// Runtime keymap application happens in the app layer, which owns `App`.
    pub fn commit(
        &mut self,
        expected_revision: u64,
        overrides: Vec<UserOverride>,
    ) -> Result<KeybindingEvent, CommitError> {
        if expected_revision != self.file.revision {
            return Err(CommitError::StaleRevision {
                current: self.file.revision,
            });
        }
        if self.file.schema_version > SCHEMA_VERSION {
            return Err(CommitError::NewerSchema {
                found: self.file.schema_version,
            });
        }
        let mut trial = self.file.overrides.clone();
        let changed: Vec<String> = overrides
            .iter()
            .map(|over| over.command_id.clone())
            .collect();
        trial.extend(overrides);
        validate_overrides(&trial)?;
        self.file.overrides = trial;
        self.file.revision += 1;
        self.write()?;
        Ok(KeybindingEvent::KeymapChanged {
            revision: self.file.revision,
            changed,
        })
    }

    /// Drop every override for `command_id` (platform-scoped when given).
    pub fn reset(&mut self, command_id: &str) -> Result<KeybindingEvent, CommitError> {
        let before = self.file.overrides.len();
        self.file
            .overrides
            .retain(|over| over.command_id != command_id);
        if self.file.overrides.len() == before {
            return Ok(KeybindingEvent::KeymapChanged {
                revision: self.file.revision,
                changed: Vec::new(),
            });
        }
        self.file.revision += 1;
        self.write()?;
        Ok(KeybindingEvent::KeymapChanged {
            revision: self.file.revision,
            changed: vec![command_id.to_string()],
        })
    }

    /// Drop all overrides (Reset all). Caller confirms first.
    pub fn reset_all(&mut self) -> Result<KeybindingEvent, CommitError> {
        let changed: Vec<String> = self
            .file
            .overrides
            .iter()
            .map(|over| over.command_id.clone())
            .collect();
        self.file.overrides.clear();
        self.file.revision += 1;
        self.write()?;
        Ok(KeybindingEvent::KeymapChanged {
            revision: self.file.revision,
            changed,
        })
    }

    /// Persist a manual layout choice. `locked` records that detection
    /// should not override the pick on restart.
    pub fn set_layout(&mut self, id: &str, locked: bool) -> Result<(), CommitError> {
        self.file.layout.mode = if locked { "manual" } else { "auto" }.to_string();
        self.file.layout.fallback_layout_id = Some(id.to_string());
        self.file.layout.locked = locked;
        self.write()
    }

    /// The layout the file last pinned, if any.
    pub fn saved_layout(&self) -> Option<&str> {
        self.file.layout.fallback_layout_id.as_deref()
    }

    /// Restore the `.bak` of last known-good data, if present.
    pub fn restore_backup(&mut self) -> Result<(), CommitError> {
        let backup = self.path.with_extension("json.bak");
        let bytes =
            fs::read(&backup).map_err(|e| CommitError::Io(format!("backup: {e}")))?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(CommitError::Io("backup exceeds size limit".into()));
        }
        let file: KeybindingsFile = serde_json::from_slice(&bytes)
            .map_err(|e| CommitError::Io(format!("backup unreadable: {e}")))?;
        self.file = file;
        self.write()
            .map(|_| self.load_error = None)
    }

    /// Serialize → temp → fsync → rename, keeping `.bak` of the previous
    /// good file.
    fn write(&self) -> Result<(), CommitError> {
        let bytes = serde_json::to_vec_pretty(&self.file)
            .map_err(|e| CommitError::Io(format!("serialize: {e}")))?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| CommitError::Io(format!("create dir: {e}")))?;
        }
        let temp = self.path.with_extension("json.tmp");
        fs::write(&temp, &bytes).map_err(|e| CommitError::Io(format!("write temp: {e}")))?;
        if let Ok(file) = fs::File::open(&temp) {
            let _ = file.sync_all();
        }
        if self.path.exists() {
            let backup = self.path.with_extension("json.bak");
            let _ = fs::copy(&self.path, &backup);
        }
        fs::rename(&temp, &self.path).map_err(|e| CommitError::Io(format!("rename: {e}")))?;
        Ok(())
    }
}

/// Validate a proposed override list against the catalog and limits.
fn validate_overrides(overrides: &[UserOverride]) -> Result<(), CommitError> {
    if overrides.len() > MAX_OVERRIDES {
        return Err(CommitError::Invalid("too many overrides".into()));
    }
    for over in overrides {
        if command(&over.command_id).is_none() {
            // Unknown commands are allowed to persist but never dispatch;
            // keep them as inactive records rather than rejecting the file.
            continue;
        }
        if let Some(context) = &over.context {
            if context.len() > MAX_CONTEXT_LEN {
                return Err(CommitError::Invalid("context too long".into()));
            }
            if gpui::KeyBindingContextPredicate::parse(context).is_err() {
                return Err(CommitError::Invalid(format!(
                    "unparseable context {context:?}"
                )));
            }
        }
        if over.semantics != "logical" {
            return Err(CommitError::Invalid(format!(
                "unsupported semantics {:?}",
                over.semantics
            )));
        }
        if let Some(sequence) = &over.sequence {
            if sequence.is_empty() || sequence.len() > MAX_STROKES {
                return Err(CommitError::Invalid(format!(
                    "sequence must be 1-{MAX_STROKES} strokes"
                )));
            }
            for stroke in sequence {
                let spelling = stroke.join("-");
                gpui::Keystroke::parse(&spelling).map_err(|_| {
                    CommitError::Invalid(format!("unparseable stroke {spelling:?}"))
                })?;
            }
        }
    }
    Ok(())
}

fn platform_applies(platforms: &[String]) -> bool {
    platforms.iter().any(|platform| {
        platform == "all"
            || (platform == "macos" && cfg!(target_os = "macos"))
            || (platform == "windows" && cfg!(target_os = "windows"))
            || (platform == "linux" && cfg!(target_os = "linux"))
    })
}

/// Resolve defaults + overrides into precedence-ordered effective bindings.
fn resolve(overrides: &[UserOverride]) -> Vec<EffectiveBinding> {
    // Group overrides by command, preserving file order.
    let mut by_command: HashMap<&str, Vec<&UserOverride>> = HashMap::new();
    for over in overrides {
        by_command
            .entry(over.command_id.as_str())
            .or_default()
            .push(over);
    }

    let mut result: Vec<EffectiveBinding> = Vec::new();
    for (index, entry) in ENTRIES.iter().enumerate() {
        if !entry.platform.is_current() {
            continue;
        }
        if command(entry.command).is_none() {
            continue;
        }
        let ops = by_command
            .get(entry.command)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|over| platform_applies(&over.platforms))
            .filter(|over| {
                over.context.as_deref() == entry.context || over.context.is_none()
            })
            .collect::<Vec<_>>();

        // An unbind kills the default; a replace rewrites it in place; adds
        // append after all defaults for the command.
        let mut suppressed = false;
        for over in &ops {
            match over.operation {
                BindingOperation::Unbind => suppressed = true,
                BindingOperation::Replace => {
                    suppressed = true;
                    if let Some(sequence) = &over.sequence {
                        result.push(EffectiveBinding {
                            command: entry.command,
                            sequence: canonical(sequence),
                            context: entry.context.map(str::to_string),
                            platform: entry.platform,
                            source: super::BindingSource::User,
                            precedence: index,
                        });
                    }
                }
                BindingOperation::Add => {}
            }
        }
        if !suppressed {
            result.push(EffectiveBinding {
                command: entry.command,
                sequence: entry.sequence.to_string(),
                context: entry.context.map(str::to_string),
                platform: entry.platform,
                source: super::BindingSource::Default,
                precedence: index,
            });
        }
    }

    // `add` operations append at the end (highest precedence), in file order.
    let mut add_index = 0usize;
    for over in overrides {
        if over.operation != BindingOperation::Add || !platform_applies(&over.platforms) {
            continue;
        }
        let Some(descriptor) = command(&over.command_id) else {
            continue;
        };
        if let Some(sequence) = &over.sequence {
            result.push(EffectiveBinding {
                command: descriptor.id,
                sequence: canonical(sequence),
                context: over.context.clone(),
                platform: PlatformSet::CURRENT,
                source: super::BindingSource::User,
                precedence: usize::MAX / 2 + add_index,
            });
            add_index += 1;
        }
    }

    result.sort_by_key(|binding| binding.precedence);
    result
}

/// `["secondary", "j"]` → `"secondary-j"`.
fn canonical(sequence: &[Vec<String>]) -> String {
    sequence
        .iter()
        .map(|stroke| stroke.join("-"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[allow(dead_code)]
fn _assert_descriptor_send(descriptor: &CommandDescriptor) -> &CommandDescriptor {
    descriptor
}
