//! Worker-prepared file listings shared by composer surfaces and the finder.

use std::sync::{Arc, OnceLock};

use waku_client::persistence::BackgroundOwned;

use crate::composer_complete::FileEntry;

pub(super) type SharedFileIndex = Arc<BackgroundOwned<FileIndex>>;

pub(super) struct FileIndex {
    pub(super) entries: Vec<FileEntry>,
    pub(super) files_only: Vec<usize>,
}

impl FileIndex {
    /// Build on the listing worker, before any UI handoff. The file finder
    /// shares this projection rather than copying paths when an index lands.
    pub(super) fn new(entries: Vec<FileEntry>) -> Self {
        let files_only = entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| (!entry.is_dir).then_some(index))
            .collect();
        Self {
            entries,
            files_only,
        }
    }
}

pub(super) fn empty() -> SharedFileIndex {
    static EMPTY: OnceLock<SharedFileIndex> = OnceLock::new();
    Arc::clone(EMPTY.get_or_init(|| Arc::new(BackgroundOwned::new(FileIndex::new(Vec::new())))))
}
