use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::model::{AgentSession, MessageRole, SessionStatus};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ComposerDraftAttachment {
    #[ts(type = "string")]
    pub path: PathBuf,
    pub mention: String,
    pub name: String,
    pub is_dir: bool,
    pub is_image: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_reference: Option<String>,
    /// Leading characters of a pasted-text attachment, kept so a restored
    /// draft's chip can still offer its hover preview. `Some` doubles as the
    /// pasted-text marker; see [`crate::model::MessageAttachment::pasted_text_preview`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pasted_text_preview: Option<String>,
    /// Task the attachment references instead of a file; see
    /// [`crate::model::MessageAttachment::session_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
}

/// One painted element's slice of an annotated passage, keyed the way the
/// renderer keys text elements: a `message-{id}` or `file:{path}` row plus the
/// element's index within that row. `start`/`end` are byte offsets into
/// `text`, which snapshots the element's flat text at selection time so the
/// quote survives later edits to the message or file.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ComposerDraftAnnotationSpan {
    pub row: String,
    pub index: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub block_break: bool,
}

/// Right-panel file provenance for a draft annotation: the workspace-relative
/// path, the byte range the pinned highlight covers in the file's text, and
/// the 1-based lines covering it at selection time for the prompt's
/// `[Selected lines N-M]` marker. `source` snapshots those bytes when the pin
/// was made on the rendered markdown preview, where the span text holds the
/// rendered passage rather than the file's own slice.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ComposerDraftFileAnnotation {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub start_line: usize,
    pub end_line: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// A commented highlight staged with the draft — a passage of an assistant
/// message or a file-editor selection plus the user's comment. `id` persists
/// so creation order survives a save and a session's next id stays above
/// everything restored.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ComposerDraftAnnotation {
    pub id: u64,
    /// The message the spans were taken from. Nil when `file` is set.
    #[ts(type = "string")]
    pub message_id: Uuid,
    pub spans: Vec<ComposerDraftAnnotationSpan>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<ComposerDraftFileAnnotation>,
}

/// Client-local metadata for an inline atom while a composer draft is being
/// moved between targets. The range points into the expanded draft text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComposerDraftInlineAtom {
    pub offset: usize,
    pub length: usize,
    pub revision: Uuid,
    pub kind: ComposerDraftInlineAtomKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComposerDraftInlineAtomKind {
    PastedText {
        text: String,
        paste_category: Option<String>,
    },
    SessionRef { session_id: Uuid, title: String },
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ComposerDraft {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<ComposerDraftAttachment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<ComposerDraftAnnotation>,
    /// Editor-only chip identity. Kept while drafts move in memory, but never
    /// serialized: persisted and cross-device drafts remain plain prompt text.
    #[serde(skip)]
    #[ts(skip)]
    pub inline_atoms: Vec<ComposerDraftInlineAtom>,
}

impl ComposerDraft {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
            && self.attachments.is_empty()
            && self.annotations.is_empty()
            && self.inline_atoms.is_empty()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ComposerDrafts {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub new_sessions: HashMap<Uuid, ComposerDraft>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub sessions: HashMap<Uuid, ComposerDraft>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComposerDraftKey {
    NewSession(Uuid),
    Session(Uuid),
}

/// Wire-safe identity for one independently persisted composer draft.
///
/// Draft updates are keyed so multiple connected clients cannot overwrite
/// unrelated drafts by sending stale whole-file snapshots.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ComposerDraftTarget {
    NewSession {
        #[ts(type = "string")]
        project_id: Uuid,
    },
    Session {
        #[ts(type = "string")]
        session_id: Uuid,
    },
}

impl From<ComposerDraftKey> for ComposerDraftTarget {
    fn from(key: ComposerDraftKey) -> Self {
        match key {
            ComposerDraftKey::NewSession(project_id) => Self::NewSession { project_id },
            ComposerDraftKey::Session(session_id) => Self::Session { session_id },
        }
    }
}

impl From<ComposerDraftTarget> for ComposerDraftKey {
    fn from(target: ComposerDraftTarget) -> Self {
        match target {
            ComposerDraftTarget::NewSession { project_id } => Self::NewSession(project_id),
            ComposerDraftTarget::Session { session_id } => Self::Session(session_id),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ComposerDraftChange {
    pub target: ComposerDraftTarget,
    /// `None` removes the target. Empty drafts are normalized to removal too.
    pub draft: Option<ComposerDraft>,
}

impl ComposerDraftKey {
    pub fn for_session(session: &AgentSession) -> Self {
        if session.has_started() {
            Self::Session(session.id)
        } else {
            Self::NewSession(session.project_id)
        }
    }
}

impl ComposerDrafts {
    pub fn get_for(&self, session: &AgentSession) -> Option<&ComposerDraft> {
        self.get(ComposerDraftKey::for_session(session))
    }

    pub fn get(&self, key: ComposerDraftKey) -> Option<&ComposerDraft> {
        match key {
            ComposerDraftKey::NewSession(project_id) => self.new_sessions.get(&project_id),
            ComposerDraftKey::Session(session_id) => self.sessions.get(&session_id),
        }
    }

    pub fn set(&mut self, key: ComposerDraftKey, draft: ComposerDraft) -> bool {
        let (drafts, id) = match key {
            ComposerDraftKey::NewSession(project_id) => (&mut self.new_sessions, project_id),
            ComposerDraftKey::Session(session_id) => (&mut self.sessions, session_id),
        };
        if draft.is_empty() {
            drafts.remove(&id).is_some()
        } else if drafts.get(&id) == Some(&draft) {
            false
        } else {
            drafts.insert(id, draft);
            true
        }
    }

    pub fn remove(&mut self, key: ComposerDraftKey) -> bool {
        match key {
            ComposerDraftKey::NewSession(project_id) => {
                self.new_sessions.remove(&project_id).is_some()
            }
            ComposerDraftKey::Session(session_id) => self.sessions.remove(&session_id).is_some(),
        }
    }

    pub fn move_to_empty(
        &mut self,
        source: ComposerDraftKey,
        destination: ComposerDraftKey,
    ) -> bool {
        if source == destination || self.get(destination).is_some_and(|draft| !draft.is_empty()) {
            return false;
        }
        let Some(draft) = self.get(source).cloned() else {
            return false;
        };
        self.remove(source);
        self.set(destination, draft)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct SessionMessageMatch {
    pub session_id: Uuid,
    pub source: MessageRole,
    pub snippet: String,
}

/// Which slice of the message store a transcript search scans. The surfaces
/// stay complementary: the command palette searches active tasks, the
/// Archived settings page searches the archive.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SessionMessageSearchScope {
    /// Non-archived sessions only.
    #[default]
    Active,
    /// Archived sessions only.
    Archived,
    /// Both — selected by the `archived:any` filter token.
    Any,
}

/// A session-message search after its `field:value` filters are lifted out
/// of the raw query.
///
/// The grammar is shared by the command palette and `goddard-agent search`:
/// `project:<name-or-id>`, `status:<status>`, `archived:<true|any|false>`,
/// and `limit:<n>` are filters. Any other token — including a `field:` token
/// whose name or value is not recognized — stays in `text`, so a stray
/// qualifier degrades to a literal search instead of an error. Values may be
/// double-quoted to hold whitespace (`project:"my app"`); repeated
/// `project:`/`status:` tokens union, `archived:`/`limit:` take the last
/// value, and different filters intersect.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionMessageSearchQuery {
    /// The remaining free text: one case-insensitive substring needle.
    pub text: String,
    /// `project:` values as typed — the caller resolves them to project ids.
    pub projects: Vec<String>,
    /// `status:` values; `busy` expands to [`SessionStatus::is_busy`]'s set.
    pub statuses: Vec<SessionStatus>,
    /// `archived:` override; `None` keeps the caller's scope.
    pub scope: Option<SessionMessageSearchScope>,
    /// `limit:` override; `None` keeps the caller's cap.
    pub limit: Option<usize>,
}

impl SessionMessageSearchQuery {
    /// Whether anything — text or filter — constrains the search.
    pub fn is_blank(&self) -> bool {
        self.text.is_empty()
            && self.projects.is_empty()
            && self.statuses.is_empty()
            && self.scope.is_none()
            && self.limit.is_none()
    }
}

/// Split a query into whitespace-separated tokens without breaking inside
/// double quotes — `project:"my app"` stays one token, as does the phrase
/// `"my app"`.
fn session_search_tokens(query: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut rest = query;
    while let Some(start) = rest.find(|ch: char| !ch.is_whitespace()) {
        rest = &rest[start..];
        let mut end = rest.len();
        let mut quoted = false;
        for (index, ch) in rest.char_indices() {
            match ch {
                '"' => quoted = !quoted,
                ch if ch.is_whitespace() && !quoted => {
                    end = index;
                    break;
                }
                _ => {}
            }
        }
        tokens.push(&rest[..end]);
        rest = &rest[end..];
    }
    tokens
}

/// Strip the quotes of a `"…"`-quoted value; a bare value is returned as is.
fn unquote(value: &str) -> &str {
    match value.strip_prefix('"') {
        Some(inner) => inner.strip_suffix('"').unwrap_or(inner),
        None => value,
    }
}

/// Apply one `name:value` token to `parsed`, returning whether the token was
/// a recognized filter. A known field with an unusable value returns `false`
/// so the token falls back to literal search text.
fn apply_session_search_filter(
    name: &str,
    value: &str,
    parsed: &mut SessionMessageSearchQuery,
) -> bool {
    let value = unquote(value);
    match name.to_ascii_lowercase().as_str() {
        "project" if !value.is_empty() => {
            parsed.projects.push(value.to_owned());
            true
        }
        "status" => {
            let statuses: &[SessionStatus] = match value.to_ascii_lowercase().as_str() {
                "idle" => &[SessionStatus::Idle],
                "connecting" => &[SessionStatus::Connecting],
                "working" => &[SessionStatus::Working],
                "waiting" => &[SessionStatus::Waiting],
                "background" => &[SessionStatus::Background],
                "failed" => &[SessionStatus::Failed],
                "busy" => &[
                    SessionStatus::Connecting,
                    SessionStatus::Working,
                    SessionStatus::Waiting,
                    SessionStatus::Background,
                ],
                _ => return false,
            };
            for status in statuses {
                if !parsed.statuses.contains(status) {
                    parsed.statuses.push(*status);
                }
            }
            true
        }
        "archived" => {
            parsed.scope = Some(match value.to_ascii_lowercase().as_str() {
                "true" => SessionMessageSearchScope::Archived,
                "any" | "all" => SessionMessageSearchScope::Any,
                "false" => SessionMessageSearchScope::Active,
                _ => return false,
            });
            true
        }
        "limit" => match value.parse::<usize>() {
            Ok(limit) => {
                parsed.limit = Some(limit);
                true
            }
            Err(_) => false,
        },
        _ => false,
    }
}

/// Resolve a `project:` filter value to a project id — the literal id, or a
/// case-insensitive name match.
pub fn resolve_named_search_project(
    projects: &[crate::model::Project],
    value: &str,
) -> Option<Uuid> {
    if let Ok(id) = Uuid::parse_str(value) {
        return projects
            .iter()
            .any(|project| project.id == id)
            .then_some(id);
    }
    projects
        .iter()
        .find(|project| project.name.eq_ignore_ascii_case(value))
        .map(|project| project.id)
}

pub fn parse_session_message_search(query: &str) -> SessionMessageSearchQuery {
    let mut parsed = SessionMessageSearchQuery::default();
    let mut text = String::new();
    for raw in session_search_tokens(query) {
        let consumed = match raw.split_once(':') {
            Some((name, value))
                if !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphabetic()) =>
            {
                apply_session_search_filter(name, value, &mut parsed)
            }
            _ => false,
        };
        if consumed {
            continue;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(unquote(raw));
    }
    parsed.text = text;
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_query_stays_one_literal_needle() {
        let parsed = parse_session_message_search("  retry   logic  ");
        assert_eq!(parsed.text, "retry logic");
        assert!(parsed.projects.is_empty());
        assert!(parsed.statuses.is_empty());
        assert_eq!(parsed.scope, None);
        assert_eq!(parsed.limit, None);
    }

    #[test]
    fn filters_lift_out_of_the_text() {
        let parsed =
            parse_session_message_search("project:goddard status:idle archived:any limit:5 retry");
        assert_eq!(parsed.text, "retry");
        assert_eq!(parsed.projects, ["goddard"]);
        assert_eq!(parsed.statuses, [SessionStatus::Idle]);
        assert_eq!(parsed.scope, Some(SessionMessageSearchScope::Any));
        assert_eq!(parsed.limit, Some(5));
    }

    #[test]
    fn busy_expands_to_the_busy_status_set() {
        let parsed = parse_session_message_search("status:busy");
        assert_eq!(
            parsed.statuses,
            [
                SessionStatus::Connecting,
                SessionStatus::Working,
                SessionStatus::Waiting,
                SessionStatus::Background,
            ]
        );
        assert!(parsed.statuses.iter().all(|status| status.is_busy()));
    }

    #[test]
    fn repeated_filters_union_and_scalars_take_the_last_value() {
        let parsed = parse_session_message_search(
            "status:idle status:failed status:idle project:a project:b archived:true archived:false",
        );
        assert_eq!(
            parsed.statuses,
            [SessionStatus::Idle, SessionStatus::Failed]
        );
        assert_eq!(parsed.projects, ["a", "b"]);
        assert_eq!(parsed.scope, Some(SessionMessageSearchScope::Active));
    }

    #[test]
    fn quoted_values_and_phrases_hold_whitespace() {
        let parsed = parse_session_message_search("project:\"my app\" \"the fix\" tail");
        assert_eq!(parsed.projects, ["my app"]);
        assert_eq!(parsed.text, "the fix tail");
    }

    #[test]
    fn unknown_fields_and_bad_values_fall_back_to_literal_text() {
        let parsed =
            parse_session_message_search("status:bogus frobnicate:x limit:nope http://a.b");
        assert_eq!(
            parsed.text,
            "status:bogus frobnicate:x limit:nope http://a.b"
        );
        assert!(parsed.statuses.is_empty());
        assert_eq!(parsed.limit, None);
    }

    #[test]
    fn a_filter_only_query_is_not_blank() {
        assert!(parse_session_message_search("status:idle").is_blank() == false);
        assert!(parse_session_message_search("  ").is_blank());
        assert!(parse_session_message_search("").is_blank());
    }
}
