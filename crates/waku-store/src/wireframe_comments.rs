//! Discussion writes use their own immediate transaction. Transcript saves do
//! not touch this table, and concurrent replies cannot replace each other.

use std::collections::BTreeMap;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::persistence::StateStore;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use waku_protocol::wireframe_comments::*;

const MAX_DISCUSSION_BYTES: usize = 1_048_576;
const MAX_THREADS: usize = 200;
const MAX_MESSAGES: usize = 128;
const PAGE_SIZE: usize = 10;

#[derive(Default, Serialize, Deserialize)]
struct Discussion {
    threads: Vec<WireframeCommentThread>,
    seen: BTreeMap<String, BTreeMap<Uuid, usize>>,
    mutations: BTreeMap<String, String>,
}

/// Metadata verified by the daemon against the task's workspace.
pub struct CommentDocument<'a> {
    pub task: Uuid,
    pub id: &'a str,
    pub path: &'a str,
    pub revision: &'a str,
    pub label: &'a str,
    /// The daemon verified that the old registered file no longer exists.
    pub allow_path_move: bool,
    pub previous_path: Option<&'a str>,
}

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}
fn err(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

impl StateStore {
    /// Retry recognition lets the daemon recover an accepted creation even
    /// when its anchor was removed before the caller received the response.
    pub fn wireframe_comment_retry(
        &self,
        task: Uuid,
        document: &str,
        actor: &str,
        key: &str,
    ) -> io::Result<bool> {
        let stored: Option<String> = self
            .open()?
            .query_row(
                "SELECT data FROM wireframe_discussions WHERE id=?1",
                [format!("{task}:{document}")],
                |row| row.get(0),
            )
            .optional()
            .map_err(err)?;
        let Some(data) = stored else {
            return Ok(false);
        };
        let discussion: Discussion = serde_json::from_str(&data).map_err(err)?;
        Ok(discussion.mutations.contains_key(&format!("{actor}:{key}")))
    }
    /// Current registered path, for the daemon's duplicate-document check.
    pub fn wireframe_comment_path(&self, task: Uuid, document: &str) -> io::Result<Option<String>> {
        self.open()?
            .query_row(
                "SELECT path FROM wireframe_discussions WHERE id=?1",
                [format!("{task}:{document}")],
                |row| row.get(0),
            )
            .optional()
            .map_err(err)
    }

    /// Apply one authorized operation and return a bounded discussion page.
    /// Author/reader identity and element labels must come from the daemon.
    pub fn wireframe_comments(
        &self,
        document: CommentDocument<'_>,
        actor: &str,
        author_label: &str,
        operation: WireframeCommentOperation,
    ) -> io::Result<WireframeCommentsPage> {
        let mut connection = self.open()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let id = format!("{}:{}", document.task, document.id);
        let stored: Option<(String, String)> = transaction
            .query_row(
                "SELECT path,data FROM wireframe_discussions WHERE id=?1",
                [&id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(err)?;
        if stored.as_ref().is_some_and(|(path, _)| {
            path != document.path
                && (!document.allow_path_move || document.previous_path != Some(path.as_str()))
        }) {
            return Err(invalid(
                "another file already uses this wireframe document ID",
            ));
        }
        let original_data = stored.as_ref().map(|(_, data)| data.as_str());
        let mut discussion: Discussion = original_data
            .map(|value| serde_json::from_str(value).map_err(err))
            .transpose()?
            .unwrap_or_default();
        let fingerprint = serde_json::to_string(&operation).map_err(err)?;
        let key = match &operation {
            WireframeCommentOperation::Add { key, .. }
            | WireframeCommentOperation::Reply { key, .. } => {
                if key.is_empty() || key.len() > 128 {
                    return Err(invalid("idempotency key must contain 1–128 bytes"));
                }
                Some(format!("{actor}:{key}"))
            }
            _ => None,
        };
        let retry = key.as_ref().and_then(|key| discussion.mutations.get(key));
        if retry.is_some_and(|old| old != &fingerprint) {
            return Err(invalid(
                "idempotency key already used for different comment content",
            ));
        }
        let mut offset = 0;
        let requested_thread = match &operation {
            WireframeCommentOperation::Get { thread } => Some(*thread),
            _ => None,
        };
        if retry.is_none() {
            match operation {
                WireframeCommentOperation::Get { thread } => {
                    if !discussion.threads.iter().any(|t| t.id == thread) {
                        return Err(invalid("unknown wireframe thread"));
                    }
                }
                WireframeCommentOperation::List { offset: requested } => {
                    offset = requested as usize
                }
                WireframeCommentOperation::Add {
                    screen,
                    anchor,
                    body,
                    ..
                } => {
                    if discussion.threads.len() >= MAX_THREADS {
                        return Err(invalid("wireframe has reached its 200-thread limit"));
                    }
                    let message = comment_message(actor, author_label, body)?;
                    discussion.threads.push(WireframeCommentThread {
                        id: Uuid::new_v4(),
                        screen,
                        anchor,
                        label: document.label.to_owned(),
                        revision: document.revision.to_owned(),
                        resolved: false,
                        messages: vec![message],
                        unread: false,
                    });
                    let thread = discussion.threads.last().expect("just appended thread");
                    discussion
                        .seen
                        .entry(actor.to_owned())
                        .or_default()
                        .insert(thread.id, 1);
                }
                WireframeCommentOperation::Reply { thread, body, .. } => {
                    let target = discussion
                        .threads
                        .iter_mut()
                        .find(|t| t.id == thread)
                        .ok_or_else(|| invalid("unknown wireframe thread"))?;
                    if target.messages.len() >= MAX_MESSAGES {
                        return Err(invalid("thread has reached its 128-message limit"));
                    }
                    target
                        .messages
                        .push(comment_message(actor, author_label, body)?);
                }
                WireframeCommentOperation::Read { thread, message } => {
                    let target = discussion
                        .threads
                        .iter()
                        .find(|t| t.id == thread)
                        .ok_or_else(|| invalid("unknown wireframe thread"))?;
                    let index = target
                        .messages
                        .iter()
                        .position(|m| m.id == message)
                        .ok_or_else(|| invalid("unknown wireframe comment"))?
                        + 1;
                    let seen = discussion
                        .seen
                        .entry(actor.to_owned())
                        .or_default()
                        .entry(thread)
                        .or_default();
                    *seen = (*seen).max(index);
                }
                WireframeCommentOperation::Resolve { thread, resolved } => {
                    discussion
                        .threads
                        .iter_mut()
                        .find(|t| t.id == thread)
                        .ok_or_else(|| invalid("unknown wireframe thread"))?
                        .resolved = resolved;
                }
            }
            if let Some(key) = key {
                discussion.mutations.insert(key, fingerprint);
            }
        }
        let data = serde_json::to_string(&discussion).map_err(err)?;
        if data.len() > MAX_DISCUSSION_BYTES {
            return Err(invalid("wireframe discussion exceeds the 1 MiB limit"));
        }
        transaction.execute("INSERT INTO wireframe_discussions(id,task_id,document_id,path,revision,data) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET path=excluded.path,revision=excluded.revision,data=excluded.data WHERE wireframe_discussions.path != excluded.path OR wireframe_discussions.revision != excluded.revision OR wireframe_discussions.data != excluded.data", params![id, document.task.to_string(), document.id, document.path, document.revision, data]).map_err(err)?;
        transaction.commit().map_err(err)?;
        let next_offset = (requested_thread.is_none()
            && discussion.threads.len() > offset.saturating_add(PAGE_SIZE))
        .then_some((offset.saturating_add(PAGE_SIZE)) as u32);
        let threads = discussion
            .threads
            .into_iter()
            .filter(|thread| requested_thread.is_none_or(|id| thread.id == id))
            .skip(offset)
            .take(PAGE_SIZE)
            .map(|mut thread| {
                let seen = discussion
                    .seen
                    .get(actor)
                    .and_then(|seen| seen.get(&thread.id))
                    .copied()
                    .unwrap_or(0);
                thread.unread = thread
                    .messages
                    .iter()
                    .skip(seen)
                    .any(|message| message.author_id != actor);
                thread
            })
            .collect();
        Ok(WireframeCommentsPage {
            document: document.id.to_owned(),
            path: document.path.to_owned(),
            revision: document.revision.to_owned(),
            threads,
            next_offset,
        })
    }
}

fn comment_message(actor: &str, label: &str, body: String) -> io::Result<WireframeCommentMessage> {
    if body.trim().is_empty() || body.len() > 16_384 {
        return Err(invalid("comments must contain 1–16384 UTF-8 bytes"));
    }
    Ok(WireframeCommentMessage {
        id: Uuid::new_v4(),
        author_id: actor.to_owned(),
        author_label: label.to_owned(),
        body,
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("wireframe-comments-{}", Uuid::new_v4())))
        }
        fn store(&self) -> StateStore {
            StateStore::new(self.0.join("app.db"))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn run(
        store: &StateStore,
        task: Uuid,
        actor: &str,
        operation: WireframeCommentOperation,
    ) -> io::Result<WireframeCommentsPage> {
        store.wireframe_comments(
            CommentDocument {
                task,
                id: "proposal",
                path: "proposal.wireframe.html",
                revision: "rev-1",
                label: "Sidebar",
                allow_path_move: false,
                previous_path: None,
            },
            actor,
            if actor == "user" { "You" } else { "Agent" },
            operation,
        )
    }
    fn add() -> WireframeCommentOperation {
        WireframeCommentOperation::Add {
            screen: "tasks".into(),
            anchor: Some("sidebar".into()),
            body: "Keep navigation visible".into(),
            key: "sidebar-rationale".into(),
        }
    }

    #[test]
    fn restart_and_read_preserve_the_thread_and_marker_contract() {
        let fixture = Fixture::new();
        let task = Uuid::new_v4();
        let created = run(&fixture.store(), task, "agent", add()).unwrap();
        let thread = &created.threads[0];
        let loaded = run(
            &fixture.store(),
            task,
            "user",
            WireframeCommentOperation::List { offset: 0 },
        )
        .unwrap();
        assert!(loaded.threads[0].unread);
        let read = run(
            &fixture.store(),
            task,
            "user",
            WireframeCommentOperation::Read {
                thread: thread.id,
                message: thread.messages[0].id,
            },
        )
        .unwrap();
        assert_eq!(read.threads.len(), 1);
        assert!(!read.threads[0].unread);
        assert!(!read.threads[0].resolved);
        run(
            &fixture.store(),
            task,
            "agent",
            WireframeCommentOperation::Reply {
                thread: thread.id,
                body: "New information".into(),
                key: "new-information".into(),
            },
        )
        .unwrap();
        let replied = run(
            &fixture.store(),
            task,
            "user",
            WireframeCommentOperation::Reply {
                thread: thread.id,
                body: "My reply to the earlier comment".into(),
                key: "user-reply".into(),
            },
        )
        .unwrap();
        assert!(
            replied.threads[0].unread,
            "replying must not mark a concurrent incoming message read"
        );
        let other_task = run(
            &fixture.store(),
            Uuid::new_v4(),
            "user",
            WireframeCommentOperation::List { offset: 0 },
        )
        .unwrap();
        assert!(other_task.threads.is_empty());
    }

    #[test]
    fn retries_and_concurrent_replies_never_erase_or_duplicate_discussion() {
        let fixture = Fixture::new();
        let store = Arc::new(fixture.store());
        let task = Uuid::new_v4();
        let first = run(&store, task, "agent", add()).unwrap();
        let retried = run(&store, task, "agent", add()).unwrap();
        assert_eq!(first.threads[0].id, retried.threads[0].id);
        assert_eq!(retried.threads.len(), 1);
        let thread = first.threads[0].id;
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let store = store.clone();
                std::thread::spawn(move || {
                    run(
                        &store,
                        task,
                        "user",
                        WireframeCommentOperation::Reply {
                            thread,
                            body: format!("Reply {i}"),
                            key: format!("reply-{i}"),
                        },
                    )
                    .unwrap()
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let loaded = run(
            &store,
            task,
            "user",
            WireframeCommentOperation::List { offset: 0 },
        )
        .unwrap();
        assert_eq!(loaded.threads[0].messages.len(), 5);
        assert!(
            run(
                &store,
                task,
                "agent",
                WireframeCommentOperation::Add {
                    screen: "tasks".into(),
                    anchor: Some("sidebar".into()),
                    body: "Different content".into(),
                    key: "sidebar-rationale".into()
                }
            )
            .is_err()
        );
        assert_eq!(
            run(
                &store,
                task,
                "user",
                WireframeCommentOperation::List { offset: 0 }
            )
            .unwrap()
            .threads[0]
                .messages
                .len(),
            5
        );
    }
}
