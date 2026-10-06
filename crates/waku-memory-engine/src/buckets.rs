//! Append-only named memory buckets with deterministic chronological summaries.
//!
//! Bucket identity and grants are managed by the caller. This module only
//! checks the supplied access snapshot and never creates a bucket during a
//! content operation.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

const OVERVIEW_SEGMENT_LIMIT: usize = 24;
const RECENT_NOTE_COUNT: usize = 4;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct BucketAccess {
    /// Buckets the authority knows about. Content calls cannot create them.
    pub buckets: Vec<Bucket>,
    pub grants: Vec<BucketGrant>,
    pub boss: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BucketGrant {
    pub bucket_id: String,
    pub principal_id: String,
    pub read: bool,
    pub insert: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NoteKind {
    Fact,
    Observation,
    Question,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BucketNote {
    pub id: String,
    pub bucket_id: String,
    pub sequence: u64,
    pub kind: NoteKind,
    pub text: String,
    pub retry_key: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub bucket_id: String,
    pub start: u64,
    pub end: u64,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SummarySource {
    Note { note: BucketNote },
    Summary { summary: Summary },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompressionRequest {
    pub bucket_id: String,
    pub start: u64,
    pub end: u64,
    pub sources: Vec<SummarySource>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OverviewItem {
    Note { note: BucketNote },
    Summary { summary: Summary },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub bucket_id: String,
    pub items: Vec<OverviewItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compression: Option<CompressionRequest>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InsertResult {
    pub note: BucketNote,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compression: Option<CompressionRequest>,
}

pub struct BucketStore {
    root: PathBuf,
}

impl BucketStore {
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Create a bucket's storage after its owner has authorized creation.
    pub fn create_bucket(&self, bucket: &Bucket) -> Result<()> {
        validate_id(&bucket.id)?;
        if bucket.name.trim().is_empty() || bucket.name.len() > 256 {
            bail!("bucket name must be between 1 and 256 bytes")
        }
        let dir = self.bucket_dir(&bucket.id);
        fs::create_dir_all(dir.join("summaries"))?;
        let path = dir.join("BUCKET.json");
        let bytes = serde_json::to_vec_pretty(bucket)?;
        match fs::read(&path) {
            Ok(existing) if existing == bytes => Ok(()),
            Ok(_) => bail!("bucket metadata conflict for {}", bucket.id),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                write_atomic(&path, &bytes)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn list_buckets(&self) -> Result<Vec<Bucket>> {
        let root = self.root.join("buckets");
        let mut buckets = Vec::new();
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(buckets),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let path = entry?.path().join("BUCKET.json");
            match fs::read(&path) {
                Ok(bytes) => {
                    buckets.push(serde_json::from_slice(&bytes).context("invalid bucket metadata")?)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            }
        }
        buckets.sort_by(|a: &Bucket, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        Ok(buckets)
    }

    pub fn insert(
        &self,
        access: &BucketAccess,
        principal: &str,
        bucket_id: &str,
        kind: NoteKind,
        text: &str,
        retry_key: &str,
    ) -> Result<InsertResult> {
        self.require(access, principal, bucket_id, true)?;
        if text.trim().is_empty() || text.len() > 64 * 1024 {
            bail!("memory note must be between 1 byte and 64 KiB")
        }
        if retry_key.trim().is_empty() || retry_key.len() > 512 {
            bail!("memory retry key must be between 1 and 512 bytes")
        }
        self.with_bucket_lock(bucket_id, || {
            let path = self.bucket_dir(bucket_id).join("LOG.jsonl");
            let notes = read_notes(&path)?;
            let digest = hex_digest(text.as_bytes());
            if let Some(existing) = notes.iter().find(|note| note.retry_key == retry_key) {
                if hex_digest(existing.text.as_bytes()) != digest || existing.kind != kind {
                    bail!("memory retry key was already used for a different note")
                }
                let compression = self.next_compression(bucket_id)?;
                return Ok(InsertResult {
                    note: existing.clone(),
                    compression,
                });
            }
            let note = BucketNote {
                id: format!("{}-{}", notes.len() + 1, digest),
                bucket_id: bucket_id.to_owned(),
                sequence: notes.len() as u64 + 1,
                kind,
                text: text.to_owned(),
                retry_key: retry_key.to_owned(),
                created_at: now(),
            };
            let mut file = OpenOptions::new().create(true).append(true).open(path)?;
            serde_json::to_writer(&mut file, &note)?;
            file.write_all(b"\n")?;
            file.sync_data()?;
            let compression = self.next_compression(bucket_id)?;
            Ok(InsertResult { note, compression })
        })
    }

    pub fn overview(
        &self,
        access: &BucketAccess,
        principal: &str,
        bucket_id: &str,
    ) -> Result<Overview> {
        self.require(access, principal, bucket_id, false)?;
        self.with_bucket_lock(bucket_id, || {
            let notes = read_notes(&self.bucket_dir(bucket_id).join("LOG.jsonl"))?;
            let items = self.overview_items(bucket_id, &notes)?;
            let compression = self.next_compression(bucket_id)?;
            Ok(Overview {
                bucket_id: bucket_id.into(),
                items,
                compression,
            })
        })
    }

    pub fn zoom(
        &self,
        access: &BucketAccess,
        principal: &str,
        bucket_id: &str,
        start: u64,
        end: u64,
    ) -> Result<Vec<OverviewItem>> {
        self.require(access, principal, bucket_id, false)?;
        self.with_bucket_lock(bucket_id, || {
            if start == 0 || end < start {
                bail!("invalid memory range {start}–{end}")
            }
            let notes = read_notes(&self.bucket_dir(bucket_id).join("LOG.jsonl"))?;
            if end > notes.len() as u64 {
                bail!("memory range {start}–{end} is outside bucket {bucket_id}")
            }
            if start == end {
                return Ok(vec![OverviewItem::Note {
                    note: notes[start as usize - 1].clone(),
                }]);
            }
            let mid = start + (end - start) / 2;
            let mut items = Vec::with_capacity(2);
            for (a, b) in [(start, mid), (mid + 1, end)] {
                if let Some(summary) = self.read_summary(bucket_id, a, b)? {
                    items.push(OverviewItem::Summary { summary });
                } else if a == b {
                    items.push(OverviewItem::Note {
                        note: notes[a as usize - 1].clone(),
                    });
                } else {
                    bail!("summary for range {a}–{b} is not available yet")
                }
            }
            Ok(items)
        })
    }

    pub fn search(
        &self,
        access: &BucketAccess,
        principal: &str,
        bucket_id: &str,
        query: &str,
    ) -> Result<Vec<BucketNote>> {
        self.require(access, principal, bucket_id, false)?;
        if query.trim().is_empty() || query.len() > 4096 {
            bail!("memory search query must be between 1 and 4096 bytes")
        }
        let query = query.to_lowercase();
        let notes = read_notes(&self.bucket_dir(bucket_id).join("LOG.jsonl"))?;
        Ok(notes
            .into_iter()
            .filter(|note| note.text.to_lowercase().contains(&query))
            .collect())
    }

    pub fn submit_summary(
        &self,
        access: &BucketAccess,
        principal: &str,
        bucket_id: &str,
        start: u64,
        end: u64,
        text: &str,
    ) -> Result<Option<CompressionRequest>> {
        self.require(access, principal, bucket_id, true)?;
        if text.trim().is_empty() || text.len() > 16 * 1024 {
            bail!("memory summary must be between 1 byte and 16 KiB")
        }
        self.with_bucket_lock(bucket_id, || {
            let request = self.next_compression(bucket_id)?;
            if !request.as_ref().is_some_and(|r| r.start == start && r.end == end) {
                bail!("summary range {start}–{end} is not the pending compression for bucket {bucket_id}")
            }
            let summary = Summary { bucket_id: bucket_id.into(), start, end, text: text.into() };
            let path = self.summary_path(bucket_id, start, end);
            if path.exists() {
                let existing: Summary = serde_json::from_slice(&fs::read(path)?)?;
                if existing == summary {
                    return self.next_compression(bucket_id);
                }
                bail!("summary range {start}–{end} already has a different summary")
            }
            write_atomic(&path, &serde_json::to_vec_pretty(&summary)?)?;
            self.next_compression(bucket_id)
        })
    }

    fn require(
        &self,
        access: &BucketAccess,
        principal: &str,
        bucket_id: &str,
        write: bool,
    ) -> Result<()> {
        validate_id(bucket_id)?;
        if !access.buckets.iter().any(|b| b.id == bucket_id) {
            bail!("unknown memory bucket {bucket_id}")
        }
        let allowed = access.boss
            || access.grants.iter().any(|grant| {
                grant.bucket_id == bucket_id
                    && grant.principal_id == principal
                    && if write { grant.insert } else { grant.read }
            });
        if !allowed {
            bail!("memory bucket access denied: {bucket_id}")
        }
        if !self.bucket_dir(bucket_id).join("BUCKET.json").exists() {
            bail!("memory bucket storage is missing: {bucket_id}")
        }
        Ok(())
    }

    fn overview_items(&self, bucket_id: &str, notes: &[BucketNote]) -> Result<Vec<OverviewItem>> {
        if notes.is_empty() {
            return Ok(Vec::new());
        }
        let recent_start = notes.len().saturating_sub(RECENT_NOTE_COUNT) as u64 + 1;
        let mut items = Vec::new();
        let mut cursor = 1_u64;
        let end = recent_start.saturating_sub(1);
        while cursor <= end && items.len() < OVERVIEW_SEGMENT_LIMIT {
            let remaining = end - cursor + 1;
            let mut size = 1_u64;
            while size.saturating_mul(2) <= remaining {
                size *= 2;
            }
            let mut found = None;
            while size > 1 {
                if let Some(summary) = self.read_summary(bucket_id, cursor, cursor + size - 1)? {
                    found = Some(summary);
                    break;
                }
                size /= 2;
            }
            if let Some(summary) = found {
                cursor = summary.end + 1;
                items.push(OverviewItem::Summary { summary });
            } else {
                items.push(OverviewItem::Note {
                    note: notes[cursor as usize - 1].clone(),
                });
                cursor += 1;
            }
        }
        for note in notes.iter().skip(recent_start.saturating_sub(1) as usize) {
            if items.len() >= OVERVIEW_SEGMENT_LIMIT {
                break;
            }
            items.push(OverviewItem::Note { note: note.clone() });
        }
        Ok(items)
    }

    fn next_compression(&self, bucket_id: &str) -> Result<Option<CompressionRequest>> {
        let notes = read_notes(&self.bucket_dir(bucket_id).join("LOG.jsonl"))?;
        let count = notes.len() as u64;
        let mut size = 2_u64;
        while size <= count {
            let mut start = 1_u64;
            while start + size - 1 <= count {
                let end = start + size - 1;
                if self.read_summary(bucket_id, start, end)?.is_none() {
                    let half = size / 2;
                    let left = self.summary_source(bucket_id, &notes, start, start + half - 1)?;
                    let right = self.summary_source(bucket_id, &notes, start + half, end)?;
                    if let (Some(left), Some(right)) = (left, right) {
                        return Ok(Some(CompressionRequest {
                            bucket_id: bucket_id.into(),
                            start,
                            end,
                            sources: vec![left, right],
                        }));
                    }
                }
                start += size;
            }
            size *= 2;
        }
        Ok(None)
    }

    fn summary_source(
        &self,
        bucket_id: &str,
        notes: &[BucketNote],
        start: u64,
        end: u64,
    ) -> Result<Option<SummarySource>> {
        if start == end {
            return Ok(notes
                .get(start as usize - 1)
                .cloned()
                .map(|note| SummarySource::Note { note }));
        }
        Ok(self
            .read_summary(bucket_id, start, end)?
            .map(|summary| SummarySource::Summary { summary }))
    }

    fn read_summary(&self, bucket_id: &str, start: u64, end: u64) -> Result<Option<Summary>> {
        let path = self.summary_path(bucket_id, start, end);
        match fs::read(path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn summary_path(&self, bucket_id: &str, start: u64, end: u64) -> PathBuf {
        self.bucket_dir(bucket_id)
            .join("summaries")
            .join(format!("{start}-{end}.json"))
    }

    fn bucket_dir(&self, bucket_id: &str) -> PathBuf {
        self.root.join("buckets").join(bucket_id)
    }

    fn with_bucket_lock<T>(
        &self,
        bucket_id: &str,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        static LOCKS: OnceLock<Mutex<BTreeMap<PathBuf, std::sync::Arc<Mutex<()>>>>> =
            OnceLock::new();
        let lock = {
            let mut locks = LOCKS
                .get_or_init(Default::default)
                .lock()
                .expect("bucket lock map poisoned");
            locks
                .entry(self.bucket_dir(bucket_id))
                .or_insert_with(|| Default::default())
                .clone()
        };
        let _guard = lock.lock().expect("bucket lock poisoned");
        action()
    }
}

fn read_notes(path: &Path) -> Result<Vec<BucketNote>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    BufReader::new(file)
        .lines()
        .map(|line| {
            let line = line?;
            serde_json::from_str(&line).context("invalid memory bucket log entry")
        })
        .collect()
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || id == "."
        || id == ".."
    {
        bail!("invalid memory bucket id")
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("memory path has no parent")?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".tmp-{}-{}", std::process::id(), now()));
    fs::write(&tmp, bytes)?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(tmp);
            Err(error.into())
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn fixture() -> (PathBuf, BucketStore, BucketAccess) {
        let root = std::env::temp_dir().join(format!("memory-buckets-{}", Uuid::new_v4()));
        let store = BucketStore::open(root.clone()).unwrap();
        let bucket = Bucket {
            id: "project-demo".into(),
            name: "Demo project".into(),
            purpose: "Project notes".into(),
            project_id: Some("demo".into()),
        };
        store.create_bucket(&bucket).unwrap();
        let access = BucketAccess {
            buckets: vec![bucket],
            grants: vec![BucketGrant {
                bucket_id: "project-demo".into(),
                principal_id: "employee".into(),
                read: true,
                insert: true,
            }],
            boss: false,
        };
        (root, store, access)
    }

    #[test]
    fn chronological_groups_request_agent_summaries_and_zoom_to_originals() {
        let (root, store, access) = fixture();
        for index in 1..=8 {
            let mut pending = store
                .insert(
                    &access,
                    "employee",
                    "project-demo",
                    NoteKind::Fact,
                    &format!("Note {index}"),
                    &format!("retry-{index}"),
                )
                .unwrap()
                .compression;
            while let Some(next) = pending.take() {
                let summary_text = format!("Summary {}-{}", next.start, next.end);
                pending = store
                    .submit_summary(
                        &access,
                        "employee",
                        "project-demo",
                        next.start,
                        next.end,
                        &summary_text,
                    )
                    .unwrap();
            }
        }
        let overview = store.overview(&access, "employee", "project-demo").unwrap();
        assert!(overview.items.iter().any(|item| matches!(item, OverviewItem::Summary { summary } if summary.start == 1 && summary.end == 4)));
        let halves = store
            .zoom(&access, "employee", "project-demo", 1, 2)
            .unwrap();
        assert_eq!(halves.len(), 2);
        assert!(matches!(&halves[0], OverviewItem::Note { note } if note.text == "Note 1"));
        assert!(matches!(&halves[1], OverviewItem::Note { note } if note.text == "Note 2"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn access_checks_retry_deduplication_and_log_search_are_bucket_local() {
        let (root, store, access) = fixture();
        let inserted = store
            .insert(
                &access,
                "employee",
                "project-demo",
                NoteKind::Observation,
                "Useful finding",
                "once",
            )
            .unwrap();
        let retried = store
            .insert(
                &access,
                "employee",
                "project-demo",
                NoteKind::Observation,
                "Useful finding",
                "once",
            )
            .unwrap();
        assert_eq!(inserted.note.id, retried.note.id);
        assert_eq!(
            store
                .search(&access, "employee", "project-demo", "finding")
                .unwrap()
                .len(),
            1
        );
        assert!(
            store
                .overview(&access, "stranger", "project-demo")
                .unwrap_err()
                .to_string()
                .contains("denied")
        );
        assert!(
            store
                .overview(&access, "employee", "private")
                .unwrap_err()
                .to_string()
                .contains("unknown")
        );
        let _ = fs::remove_dir_all(root);
    }
}
