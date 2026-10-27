//! One physical matching job, with a replaceable latest request.

use nucleo_matcher::Matcher;

use super::super::file_index::SharedFileIndex;
use crate::composer_complete::{self, FileEntry, Scored};
use waku_client::persistence::BackgroundOwned;

#[derive(Clone)]
struct Key {
    files: SharedFileIndex,
    query: String,
}

#[derive(Default)]
pub(super) struct Matching {
    generation: u64,
    key: Option<Key>,
    pending: bool,
    running: Option<u64>,
}

pub(super) struct Request {
    generation: u64,
    key: Key,
}

pub(super) struct Output {
    pub(super) generation: u64,
    pub(super) matcher: BackgroundOwned<Matcher>,
    pub(super) results: Vec<Scored<FileEntry>>,
}

impl Matching {
    pub(super) fn request(&mut self, files: SharedFileIndex, query: &str) -> bool {
        if self
            .key
            .as_ref()
            .is_some_and(|key| key.query == query && std::sync::Arc::ptr_eq(&key.files, &files))
        {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        self.key = Some(Key {
            files,
            query: query.to_owned(),
        });
        self.pending = true;
        true
    }

    pub(super) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.key = None;
        self.pending = false;
        // Revoking UI authority does not release the physical worker slot.
    }

    pub(super) fn take(&mut self) -> Option<Request> {
        if self.running.is_some() || !self.pending {
            return None;
        }
        let key = self.key.clone()?;
        self.pending = false;
        self.running = Some(self.generation);
        Some(Request {
            generation: self.generation,
            key,
        })
    }

    pub(super) fn finish(&mut self, generation: u64) -> bool {
        if self.running != Some(generation) {
            return false;
        }
        self.running = None;
        self.key.is_some() && generation == self.generation
    }

    pub(super) fn is_pending(&self) -> bool {
        self.key.is_some() && (self.pending || self.running.is_some())
    }
}

/// Called only on the background executor; allocation, scoring and scratch
/// reclamation never run in the UI continuation, including abandoned updates.
pub(super) fn run(
    request: Request,
    matcher: Option<BackgroundOwned<Matcher>>,
) -> BackgroundOwned<Output> {
    let mut matcher = matcher
        .map(BackgroundOwned::into_inner)
        .unwrap_or_else(composer_complete::matcher);
    let files = request.key.files.get();
    let results = composer_complete::filter_files_at_indices(
        &files.entries,
        &files.files_only,
        &request.key.query,
        &mut matcher,
    );
    BackgroundOwned::new(Output {
        generation: request.generation,
        matcher: BackgroundOwned::new(matcher),
        results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::file_index::FileIndex;
    use std::sync::Arc;

    fn files(path: &str) -> SharedFileIndex {
        Arc::new(BackgroundOwned::new(FileIndex::new(vec![FileEntry {
            path: path.to_owned(),
            is_dir: false,
        }])))
    }

    #[test]
    fn rapid_queries_close_and_scope_replacement_preserve_one_current_worker() {
        let files = files("src/latest.rs");
        let mut state = Matching::default();
        assert!(state.request(Arc::clone(&files), "first"));
        let first = state.take().unwrap();
        let first_generation = first.generation;
        // Hold the external worker's completion while the UI changes input.
        let (release, ready) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            ready.recv().unwrap();
            run(first, None)
        });
        for index in 0..1_000 {
            assert!(state.request(Arc::clone(&files), &format!("edit-{index}")));
            assert!(state.take().is_none(), "a second physical job was admitted");
        }
        assert!(state.request(Arc::clone(&files), "latest"));
        release.send(()).unwrap();
        let stale = worker.join().unwrap();
        assert!(!state.finish(stale.get().generation));
        drop(stale);
        let latest = state.take().unwrap();
        let latest_generation = latest.generation;
        assert!(!state.finish(first_generation));
        assert!(
            state.take().is_none(),
            "stale completion released the new slot"
        );
        let latest = std::thread::spawn(move || run(latest, None))
            .join()
            .unwrap();
        assert_eq!(latest.get().results[0].item.path, "src/latest.rs");
        assert!(state.finish(latest.get().generation));
        assert!(!state.is_pending());
        assert!(!state.request(Arc::clone(&files), "latest"));
        assert!(state.take().is_none(), "an unchanged cache hit rematched");

        // Closing/reopening the same query revokes the first opening's result
        // while retaining its physical slot until the callback arrives.
        assert!(state.request(Arc::clone(&files), "src"));
        let before_close = state.take().unwrap();
        state.invalidate();
        assert!(!state.is_pending());
        assert!(state.request(Arc::clone(&files), "src"));
        assert!(state.take().is_none());
        assert!(!state.finish(before_close.generation));
        let reopened = state.take().unwrap();
        // Replacing a workspace with the same query cannot install old paths.
        state.invalidate();
        let replacement = self::files("different/src/new.rs");
        state.request(replacement, "src");
        assert!(state.take().is_none());
        assert!(!state.finish(reopened.generation));
        let current = state.take().unwrap();
        let current = std::thread::spawn(move || run(current, None))
            .join()
            .unwrap();
        assert_eq!(current.get().results[0].item.path, "different/src/new.rs");
        assert!(state.finish(current.get().generation));
        assert!(!state.finish(latest_generation));
        assert!(!state.is_pending());
    }
}
