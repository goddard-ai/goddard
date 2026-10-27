//! Planned-workspace listings use compact snapshot keys on the UI thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uuid::Uuid;
use waku_client::persistence::BackgroundOwned;
use waku_protocol::model::{AgentSession, Message, TranscriptBlock};
use waku_protocol::shared_history::{HistoryIdentity, SharedHistory};
use waku_protocol::workspace::ProjectFilesListing;

use super::super::file_index::{FileIndex, SharedFileIndex};
use crate::composer_complete::FileEntry;

#[derive(Clone, PartialEq, Eq)]
struct Key {
    owner: waku_client::DaemonKey,
    session: Uuid,
    root: PathBuf,
    base_ref: Option<String>,
    messages: HistoryIdentity<Message>,
    blocks: HistoryIdentity<TranscriptBlock>,
}

struct Snapshot {
    messages: SharedHistory<Message>,
    blocks: SharedHistory<TranscriptBlock>,
}

pub(super) struct Input {
    key: Key,
    snapshot: BackgroundOwned<Snapshot>,
}

pub(super) struct Request {
    generation: u64,
    input: Input,
}

impl Request {
    pub(super) fn owner(&self) -> waku_client::DaemonKey {
        self.input.key.owner
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Clone)]
pub(super) enum Listing {
    SharedBase,
    Overlay(SharedFileIndex),
}

pub(super) struct Output {
    pub(super) generation: u64,
    pub(super) listing: Listing,
}

#[derive(Default)]
pub(super) struct Preparation {
    generation: u64,
    key: Option<Key>,
    ready: Option<Listing>,
    pending: Option<Request>,
    running: Option<u64>,
}

impl Preparation {
    pub(super) fn capture(
        &self,
        session: &AgentSession,
        root: &Path,
        base_ref: Option<String>,
        owner: waku_client::DaemonKey,
    ) -> Option<Input> {
        let key = Key {
            owner,
            session: session.id,
            root: root.to_owned(),
            base_ref,
            messages: session.messages.snapshot_identity(),
            blocks: session.transcript_blocks.snapshot_identity(),
        };
        if self.key.as_ref() == Some(&key)
            && (self.ready.is_some()
                || self.pending.is_some()
                || self.running == Some(self.generation))
        {
            return None;
        }
        Some(Input {
            key,
            snapshot: BackgroundOwned::new(Snapshot {
                messages: session.messages.clone(),
                blocks: session.transcript_blocks.clone(),
            }),
        })
    }

    pub(super) fn request(&mut self, input: Input) {
        self.generation = self.generation.wrapping_add(1);
        self.key = Some(input.key.clone());
        self.ready = None;
        self.pending = Some(Request {
            generation: self.generation,
            input,
        });
    }

    pub(super) fn ready(&self) -> Option<Listing> {
        self.ready.clone()
    }

    pub(super) fn take(&mut self) -> Option<Request> {
        if self.running.is_some() {
            return None;
        }
        let request = self.pending.take()?;
        self.running = Some(request.generation);
        Some(request)
    }

    pub(super) fn finish(&mut self, generation: u64, listing: Listing) -> bool {
        if self.running != Some(generation) {
            return false;
        }
        self.running = None;
        if self.generation != generation || self.key.is_none() {
            return false;
        }
        self.ready = Some(listing);
        true
    }

    pub(super) fn abandon(&mut self, generation: u64) {
        if self.running == Some(generation) {
            self.running = None;
        }
    }

    pub(super) fn suspend(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        // A ready listing may be reused on reopen. Its weak identities do not
        // pin the transcript; an active producer retains its physical slot.
    }

    pub(super) fn invalidate_workspace(&mut self, root: &Path) {
        if self.key.as_ref().is_some_and(|key| key.root == root) {
            self.suspend();
            self.key = None;
            self.ready = None;
        }
    }

    pub(super) fn invalidate_owner(&mut self, owner: waku_client::DaemonKey) {
        if self.key.as_ref().is_some_and(|key| key.owner == owner) {
            self.suspend();
            self.key = None;
            self.ready = None;
        }
    }
}

/// Parsing, path collection and transport payload construction all happen on
/// the worker. Only the prepared shared index crosses back to the UI.
pub(super) fn run(
    request: Request,
    load: impl FnOnce(PathBuf, ProjectFilesListing) -> Vec<FileEntry>,
) -> Output {
    let snapshot = request.input.snapshot.into_inner();
    let paths = super::transcript_mention_paths(
        &snapshot.messages,
        &snapshot.blocks,
        &request.input.key.root,
        false,
    );
    // Do not retain a transcript while waiting on a remote listing or Git.
    drop(snapshot);
    if paths.is_empty() {
        // The ordinary base-ref query is already shared with the composer.
        // Do not issue a second Git/RPC listing just for this surface.
        return Output {
            generation: request.generation,
            listing: Listing::SharedBase,
        };
    }
    let listing = ProjectFilesListing::BaseRefWithUntracked {
        base_ref: request.input.key.base_ref,
        paths,
    };
    let entries = load(request.input.key.root, listing);
    Output {
        generation: request.generation,
        listing: Listing::Overlay(Arc::new(BackgroundOwned::new(FileIndex::new(entries)))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_protocol::model::ProviderKind;

    fn link(root: &Path, path: &str) -> String {
        format!(
            "[linked]({})",
            url::Url::from_file_path(root.join(path)).unwrap()
        )
    }

    fn overlay(listing: &Listing) -> &SharedFileIndex {
        match listing {
            Listing::Overlay(files) => files,
            Listing::SharedBase => panic!("expected explicit file links"),
        }
    }

    fn session(content: &str) -> AgentSession {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.messages.push(
            serde_json::from_value(serde_json::json!({
                "id":Uuid::new_v4(), "role":"assistant", "content":content,
                "created_at":1, "streaming":false
            }))
            .unwrap(),
        );
        session
    }

    fn linked_files(_: PathBuf, listing: ProjectFilesListing) -> Vec<FileEntry> {
        let ProjectFilesListing::BaseRefWithUntracked { base_ref, paths } = listing else {
            panic!("explicit transcript links were lost");
        };
        assert_eq!(base_ref.as_deref(), Some("main"));
        paths
            .into_iter()
            .map(|path| FileEntry {
                path,
                is_dir: false,
            })
            .collect()
    }

    #[test]
    fn planned_listing_coalesces_snapshots_reuses_cache_and_rejects_retired_scopes() {
        let root = std::env::temp_dir().join("goddard-file-preparation-contract");
        let mut live = session(&format!("{} bare/path.rs", link(&root, "src/one.rs")));
        let mut state = Preparation::default();
        let input = state
            .capture(
                &live,
                &root,
                Some("main".into()),
                waku_client::DaemonKey::Local,
            )
            .unwrap();
        state.request(input);
        let first = state.take().unwrap();
        let first_generation = first.generation();
        let (release, ready) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            ready.recv().unwrap();
            run(first, linked_files)
        });
        for index in 0..100 {
            live.messages[0].content = link(&root, &format!("src/version-{index:03}.rs"));
            let input = state
                .capture(
                    &live,
                    &root,
                    Some("main".into()),
                    waku_client::DaemonKey::Local,
                )
                .unwrap();
            state.request(input);
            assert!(
                state.take().is_none(),
                "more than one listing worker was admitted"
            );
        }
        release.send(()).unwrap();
        let old = worker.join().unwrap();
        assert_eq!(overlay(&old.listing).get().entries[0].path, "src/one.rs");
        assert_eq!(
            overlay(&old.listing).get().entries.len(),
            1,
            "plain paths changed the link-only policy"
        );
        assert!(!state.finish(old.generation, old.listing));
        assert!(state.ready().is_none());
        let latest = state.take().unwrap();
        let result = std::thread::spawn(move || run(latest, linked_files))
            .join()
            .unwrap();
        assert_eq!(
            overlay(&result.listing).get().entries[0].path,
            "src/version-099.rs"
        );
        assert!(state.finish(result.generation, result.listing));
        let cached = Arc::clone(overlay(&state.ready().unwrap()));
        state.suspend();
        assert!(
            state
                .capture(
                    &live,
                    &root,
                    Some("main".into()),
                    waku_client::DaemonKey::Local
                )
                .is_none()
        );
        assert!(
            state.take().is_none(),
            "reopening rescanned an unchanged transcript"
        );
        assert!(Arc::ptr_eq(&cached, overlay(&state.ready().unwrap())));

        // Same-length edits with no timestamp/count change still revoke a cache.
        live.messages[0].content = link(&root, "src/version-199.rs");
        let input = state
            .capture(
                &live,
                &root,
                Some("main".into()),
                waku_client::DaemonKey::Local,
            )
            .unwrap();
        state.request(input);
        let retired = state.take().unwrap();
        state.suspend();
        let other = root.join("other");
        live.messages[0].content = link(&other, "src/version-199.rs");
        let input = state
            .capture(
                &live,
                &other,
                Some("main".into()),
                waku_client::DaemonKey::Local,
            )
            .unwrap();
        state.request(input);
        assert!(state.take().is_none());
        assert!(!state.finish(retired.generation(), Listing::Overlay(Arc::clone(&cached))));
        let current = state.take().unwrap();
        assert!(!state.finish(first_generation, Listing::Overlay(Arc::clone(&cached))));
        assert!(
            state.take().is_none(),
            "obsolete completion released the current slot"
        );
        let result = std::thread::spawn(move || run(current, linked_files))
            .join()
            .unwrap();
        assert!(state.finish(result.generation, result.listing));
        state.invalidate_workspace(&root);
        assert!(
            state.ready().is_some(),
            "unrelated invalidation discarded the current scope"
        );
        state.invalidate_workspace(&other);
        assert!(state.ready().is_none());
        assert!(
            state
                .capture(
                    &live,
                    &other,
                    Some("main".into()),
                    waku_client::DaemonKey::Local
                )
                .is_some()
        );
        let input = state
            .capture(
                &live,
                &other,
                Some("main".into()),
                waku_client::DaemonKey::Local,
            )
            .unwrap();
        state.request(input);
        let old_host = state.take().unwrap();
        state.invalidate_owner(waku_client::DaemonKey::Local);
        let owner = waku_client::DaemonKey::Remote(Uuid::new_v4());
        let input = state
            .capture(&live, &other, Some("main".into()), owner)
            .unwrap();
        state.request(input);
        assert!(state.take().is_none());
        assert!(!state.finish(old_host.generation(), Listing::Overlay(Arc::clone(&cached))));
        let current = state.take().unwrap();
        let result = std::thread::spawn(move || run(current, linked_files))
            .join()
            .unwrap();
        assert!(state.finish(result.generation, result.listing));
        state.invalidate_owner(waku_client::DaemonKey::Local);
        assert!(state.ready().is_some());
        state.invalidate_owner(owner);
        assert!(state.ready().is_none());

        let plain = session("bare/path.rs [relative](src/relative.rs)");
        let input = state
            .capture(&plain, &root, None, waku_client::DaemonKey::Local)
            .unwrap();
        state.request(input);
        let request = state.take().unwrap();
        let result = std::thread::spawn(move || {
            run(request, |_, _| {
                panic!("an unlinked transcript issued a duplicate base-ref request")
            })
        })
        .join()
        .unwrap();
        assert!(matches!(result.listing, Listing::SharedBase));
        assert!(state.finish(result.generation, result.listing));
        state.suspend();
        assert!(
            state
                .capture(&plain, &root, None, waku_client::DaemonKey::Local)
                .is_none()
        );
        assert!(state.take().is_none());
    }
}
