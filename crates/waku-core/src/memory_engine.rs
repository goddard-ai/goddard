//! File-canonical, daemon-owned memory collections.
//!
//! Records are immutable Markdown revisions; indexes are deterministic views
//! rebuilt from those records. This module deliberately contains no Jev calls.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub version: u32,
    pub scope_id: String,
    pub daemon_id: String,
    pub kind: String,
    pub owner_id: String,
    pub acl_revision: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub principal_id: String,
    pub scope_id: String,
    #[serde(default)]
    pub collections: Vec<String>,
    pub read: bool,
    pub write: bool,
    pub grantor: String,
    pub revision: u64,
    pub expires_at: Option<u64>,
}
pub type Chunk = waku_protocol::boss::MemoryChunk;

#[derive(Clone, Debug, Default)]
pub struct Acl {
    pub scopes: Vec<Scope>,
    pub grants: Vec<Grant>,
    pub now: u64,
}
impl Acl {
    fn allows(&self, principal: &str, scope: &str, collection: &str, write: bool) -> bool {
        self.scopes
            .iter()
            .find(|s| s.scope_id == scope)
            .is_some_and(|s| {
                s.owner_id == principal
                    || self.grants.iter().any(|g| {
                        g.principal_id == principal
                            && g.scope_id == scope
                            && (g.collections.is_empty()
                                || g.collections.iter().any(|c| {
                                    collection == c || collection.starts_with(&format!("{c}/"))
                                }))
                            && (!write || g.write)
                            && (write || g.read)
                            && g.expires_at.is_none_or(|t| t > self.now)
                    })
            })
    }
}

pub struct Store {
    root: PathBuf,
}
impl Store {
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }
    fn scope_dir(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty() || id.contains('/') || id.contains('\\') || id == "." || id == ".." {
            bail!("invalid scope id")
        }
        Ok(self.root.join(id))
    }
    fn require(acl: &Acl, p: &str, s: &str, c: &str, w: bool) -> Result<()> {
        if !acl.allows(p, s, c, w) {
            bail!("memory access denied")
        }
        Ok(())
    }
    pub fn create_scope(&self, scope: &Scope) -> Result<()> {
        if scope.version != 1 {
            bail!("unsupported scope version")
        }
        let dir = self.scope_dir(&scope.scope_id)?;
        fs::create_dir_all(dir.join("collections"))?;
        // Read-only operations reach this path too; rewriting an identical
        // record would make every read a write.
        let bytes = serde_json::to_vec_pretty(scope)?;
        if fs::read(dir.join("SCOPE.json")).is_ok_and(|existing| existing == bytes) {
            return Ok(());
        }
        write_atomic(&dir.join("SCOPE.json"), &bytes)
    }
    pub fn insert(
        &self,
        acl: &Acl,
        principal: &str,
        scope: &str,
        collection: &str,
        title: &str,
        cue: &str,
        body: &str,
        source_id: &str,
    ) -> Result<Chunk> {
        Self::require(acl, principal, scope, collection, true)?;
        validate_component(collection)?;
        if body.len() > 64 * 1024 || title.len() > 512 || cue.len() > 1024 {
            bail!("memory record exceeds limit")
        }
        let dir = self
            .scope_dir(scope)?
            .join("collections")
            .join(collection)
            .join("topics")
            .join("inbox");
        fs::create_dir_all(&dir)?;
        let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
        let stable = format!(
            "{:x}",
            Sha256::digest(format!("{scope}\0{collection}\0{source_id}\0{digest}").as_bytes())
        );
        let path = dir.join(format!("{stable}.md"));
        if path.exists() {
            let existing = read_chunk_file(&path)?;
            if existing.source_digest == digest && existing.source_id == source_id {
                return Ok(existing);
            }
            bail!("memory idempotency conflict")
        }
        let chunk = Chunk {
            version: 1,
            chunk_id: stable,
            scope_id: scope.into(),
            collection_id: collection.into(),
            layer: "detail".into(),
            revision: 1,
            title: title.into(),
            cue: cue.into(),
            status: "active".into(),
            source_id: source_id.into(),
            source_digest: digest,
            created_at: unix_time(),
            body: body.into(),
        };
        write_atomic(
            &path,
            format!("---\n{}---\n{}\n", serde_yaml_frontmatter(&chunk)?, body).as_bytes(),
        )?;
        let log = self.scope_dir(scope)?.join("LOG.txt");
        use std::io::Write;
        let mut f = fs::OpenOptions::new().create(true).append(true).open(log)?;
        // JSON string encoding keeps one raw fact per physical line while
        // preserving embedded newlines without rewriting the supplied text.
        writeln!(f, "{}", serde_json::to_string(body)?)?;
        self.rebuild_indexes(scope)?;
        Ok(chunk)
    }
    pub fn rebuild_indexes(&self, scope: &str) -> Result<()> {
        let base = self.scope_dir(scope)?;
        let mut entries = Vec::new();
        let colroot = base.join("collections");
        if colroot.exists() {
            for col in fs::read_dir(&colroot)? {
                let col = col?;
                if !col.file_type()?.is_dir() {
                    continue;
                }
                let c = col.file_name().to_string_lossy().into_owned();
                let mut rows = Vec::new();
                collect_chunks(&col.path(), &mut rows)?;
                rows.sort_by(|a, b| a.title.cmp(&b.title).then(a.chunk_id.cmp(&b.chunk_id)));
                let mut text = String::from("# Collection index\n\n");
                for ch in &rows {
                    let line = format!("- [{}] {} — {}\n", ch.title, ch.cue, ch.chunk_id);
                    text.push_str(&line);
                    entries.push(format!(
                        "- {c}: {} — {} ({})\n",
                        ch.cue, ch.title, ch.chunk_id
                    ));
                }
                write_atomic(&col.path().join("INDEX.md"), text.as_bytes())?;
            }
        }
        entries.sort();
        let mut top = String::from("# Memory index\n\n");
        top.extend(entries);
        write_atomic(&base.join("INDEX.md"), top.as_bytes())?;
        Ok(())
    }
    pub fn list_index(&self, acl: &Acl, p: &str, s: &str) -> Result<String> {
        // Never read the derived scope index for a restricted principal: it
        // contains cues from every collection. Build their view from the
        // authorized collection indexes only.
        let mut visible = String::from("# Authorized memory index\n\n");
        if self.owner(acl, p, s) {
            Self::require(acl, p, s, "", false)?;
            let path = self.scope_dir(s)?.join("INDEX.md");
            if path.exists() {
                return Ok(fs::read_to_string(path)?);
            }
        }
        if acl.grants.iter().any(|g| {
            g.principal_id == p
                && g.scope_id == s
                && g.read
                && g.collections.is_empty()
                && g.expires_at.is_none_or(|t| t > acl.now)
        }) {
            let path = self.scope_dir(s)?.join("INDEX.md");
            if path.exists() {
                return Ok(fs::read_to_string(path)?);
            }
        }
        let mut collections = acl
            .grants
            .iter()
            .filter(|g| {
                g.principal_id == p
                    && g.scope_id == s
                    && g.read
                    && g.expires_at.is_none_or(|t| t > acl.now)
            })
            .flat_map(|g| g.collections.iter().cloned())
            .collect::<Vec<_>>();
        collections.sort();
        collections.dedup();
        for c in collections {
            validate_component(&c)?;
            Self::require(acl, p, s, &c, false)?;
            let path = self
                .scope_dir(s)?
                .join("collections")
                .join(c)
                .join("INDEX.md");
            if path.exists() {
                visible.push_str(&fs::read_to_string(path)?);
            }
        }
        Ok(visible)
    }
    fn owner(&self, acl: &Acl, p: &str, s: &str) -> bool {
        acl.scopes
            .iter()
            .any(|x| x.scope_id == s && x.owner_id == p)
    }
    pub fn read_chunk(&self, acl: &Acl, p: &str, s: &str, c: &str, id: &str) -> Result<Chunk> {
        Self::require(acl, p, s, c, false)?;
        validate_component(c)?;
        let mut found = Vec::new();
        collect_chunks(&self.scope_dir(s)?.join("collections").join(c), &mut found)?;
        found
            .into_iter()
            .find(|x| x.chunk_id == id)
            .context("memory chunk not found")
    }
    pub fn search(&self, acl: &Acl, p: &str, s: &str, c: &str, needle: &str) -> Result<Vec<Chunk>> {
        Self::require(acl, p, s, c, false)?;
        validate_component(c)?;
        if needle.is_empty() || needle.len() > 4096 {
            bail!("invalid memory search query")
        }
        let mut chunks = Vec::new();
        collect_chunks(&self.scope_dir(s)?.join("collections").join(c), &mut chunks)?;
        Ok(chunks
            .into_iter()
            .filter(|x| {
                x.body.contains(needle) || x.title.contains(needle) || x.cue.contains(needle)
            })
            .collect())
    }
    pub fn surface_fallback(
        &self,
        acl: &Acl,
        p: &str,
        s: &str,
        c: &str,
        limit: usize,
    ) -> Result<Vec<Chunk>> {
        Self::require(acl, p, s, c, false)?;
        validate_component(c)?;
        let mut chunks = Vec::new();
        collect_chunks(&self.scope_dir(s)?.join("collections").join(c), &mut chunks)?;
        chunks.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then(a.chunk_id.cmp(&b.chunk_id))
        });
        chunks.truncate(limit.min(100));
        Ok(chunks)
    }
    /// Descend the collection hierarchy without widening the caller's grant.
    /// `index` returns cues, `topic:<name>` returns that topic's chunks, and
    /// `chunk:<id>` fetches one detail record.
    pub fn zoom(&self, acl: &Acl, p: &str, s: &str, c: &str, target: &str) -> Result<Vec<Chunk>> {
        Self::require(acl, p, s, c, false)?;
        validate_component(c)?;
        if let Some(id) = target.strip_prefix("chunk:") {
            return Ok(vec![self.read_chunk(acl, p, s, c, id)?]);
        }
        let root = self.scope_dir(s)?.join("collections").join(c);
        let topic = target.strip_prefix("topic:").unwrap_or("inbox");
        validate_component(topic)?;
        let mut chunks = Vec::new();
        collect_chunks(&root.join("topics").join(topic), &mut chunks)?;
        chunks.sort_by(|a, b| a.title.cmp(&b.title).then(a.chunk_id.cmp(&b.chunk_id)));
        Ok(chunks)
    }
    pub fn import_folder(
        &self,
        acl: &Acl,
        p: &str,
        s: &str,
        folder: &str,
        collection: &str,
    ) -> Result<usize> {
        Self::require(acl, p, s, collection, true)?;
        let source = Path::new(folder);
        let mut files = Vec::new();
        collect_files(source, &mut files)?;
        files.sort();
        let mut imported = 0;
        for path in files {
            let bytes = fs::read(&path)?;
            let rel = path.strip_prefix(source).unwrap_or(&path).to_string_lossy();
            let text = String::from_utf8(bytes.clone());
            let Ok(body) = text else {
                let q = self.scope_dir(s)?.join("quarantine");
                fs::create_dir_all(&q)?;
                write_atomic(&q.join(format!("{}.bin", hex_digest(&bytes))), &bytes)?;
                continue;
            };
            if body.trim().is_empty() {
                continue;
            }
            let title = path
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or("Imported memory");
            let source_id = format!("import:{rel}");
            if self
                .insert(
                    acl,
                    p,
                    s,
                    collection,
                    title,
                    "Imported legacy memory",
                    &body,
                    &source_id,
                )
                .is_ok()
            {
                imported += 1
            } else {
                let q = self.scope_dir(s)?.join("quarantine");
                fs::create_dir_all(&q)?;
                write_atomic(
                    &q.join(format!("{}.txt", hex_digest(&bytes))),
                    body.as_bytes(),
                )?;
            }
        }
        Ok(imported)
    }
}
fn validate_component(x: &str) -> Result<()> {
    if x.is_empty()
        || x.split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.contains('\\'))
    {
        bail!("invalid collection id")
    }
    Ok(())
}
fn serde_yaml_frontmatter(c: &Chunk) -> Result<String> {
    Ok(format!(
        "version: {}\nchunk_id: {}\nscope_id: {}\ncollection_id: {}\nlayer: {}\nrevision: {}\ntitle: {}\ncue: {}\nstatus: {}\nsource_id: {}\nsource_digest: {}\ncreated_at: {}\n",
        c.version,
        q(&c.chunk_id),
        q(&c.scope_id),
        q(&c.collection_id),
        q(&c.layer),
        c.revision,
        q(&c.title),
        q(&c.cue),
        q(&c.status),
        q(&c.source_id),
        q(&c.source_digest),
        c.created_at
    ))
}
fn q(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}
fn read_chunk_file(p: &Path) -> Result<Chunk> {
    let text = fs::read_to_string(p).context("corrupt memory chunk encoding")?;
    let rest = text
        .strip_prefix("---\n")
        .context("corrupt memory chunk frontmatter")?;
    let (head, body) = rest
        .split_once("\n---\n")
        .context("corrupt memory chunk frontmatter")?;
    let mut v = serde_json::Map::new();
    for line in head.lines() {
        let (k, val) = line
            .split_once(": ")
            .context("corrupt memory chunk field")?;
        let value =
            serde_json::from_str(val).unwrap_or_else(|_| serde_json::Value::String(val.into()));
        v.insert(k.into(), value);
    }
    let mut c: Chunk = serde_json::from_value(v.into()).context("corrupt memory chunk metadata")?;
    c.body = body.strip_suffix('\n').unwrap_or(body).into();
    if c.version != 1
        || c.chunk_id.is_empty()
        || c.source_digest != format!("{:x}", Sha256::digest(c.body.as_bytes()))
    {
        bail!("corrupt memory chunk digest or version")
    }
    Ok(c)
}
fn collect_chunks(root: &Path, out: &mut Vec<Chunk>) -> Result<()> {
    let scope_root = root
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "collections"))
        .and_then(Path::parent)
        .unwrap_or(root);
    collect_chunks_in(root, scope_root, out)
}
fn collect_chunks_in(root: &Path, scope_root: &Path, out: &mut Vec<Chunk>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for e in fs::read_dir(root)? {
        let e = e?;
        let t = e.file_type()?;
        if t.is_symlink() {
            continue;
        }
        if t.is_dir() {
            collect_chunks_in(&e.path(), scope_root, out)?
        } else if e.path().extension().is_some_and(|x| x == "md") && e.file_name() != "INDEX.md" {
            match read_chunk_file(&e.path()) {
                Ok(chunk) => out.push(chunk),
                Err(_) => {
                    // Corrupt records are removed from the active tree but
                    // retained byte-for-byte for repair or manual recovery.
                    let bytes = fs::read(e.path())?;
                    let quarantine = scope_root.join("quarantine");
                    fs::create_dir_all(&quarantine)?;
                    let destination = quarantine.join(format!("{}.md", hex_digest(&bytes)));
                    if !destination.exists() {
                        write_atomic(&destination, &bytes)?;
                    }
                    fs::remove_file(e.path())?;
                }
            }
        }
    }
    Ok(())
}
fn collect_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for e in fs::read_dir(root)? {
        let e = e?;
        let t = e.file_type()?;
        if t.is_symlink() {
            continue;
        }
        if t.is_dir() {
            collect_files(&e.path(), out)?
        } else if t.is_file() {
            out.push(e.path())
        }
    }
    Ok(())
}
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("missing parent")?;
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)?;
    Ok(())
}
fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |x| x.as_secs())
}
fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acl_matrix_and_import_idempotency_and_corruption() {
        let t = std::env::temp_dir().join(format!("memory-{}", Uuid::new_v4()));
        let store = Store::open(t.join("store")).unwrap();
        store
            .create_scope(&Scope {
                version: 1,
                scope_id: "boss".into(),
                daemon_id: "d".into(),
                kind: "boss".into(),
                owner_id: "boss-user".into(),
                acl_revision: 1,
            })
            .unwrap();
        let acl = Acl {
            scopes: vec![Scope {
                version: 1,
                scope_id: "boss".into(),
                daemon_id: "d".into(),
                kind: "boss".into(),
                owner_id: "boss-user".into(),
                acl_revision: 1,
            }],
            grants: vec![Grant {
                principal_id: "employee".into(),
                scope_id: "boss".into(),
                collections: vec!["work".into()],
                read: true,
                write: false,
                grantor: "boss-user".into(),
                revision: 1,
                expires_at: None,
            }],
            now: unix_time(),
        };
        assert!(
            store
                .insert(
                    &acl, "employee", "boss", "private", "secret", "cue", "body", "src"
                )
                .is_err()
        );
        let private = store
            .insert(
                &acl,
                "boss-user",
                "boss",
                "private",
                "classified title",
                "classified cue",
                "classified body",
                "private-source",
            )
            .unwrap();
        let authorized_index = store.list_index(&acl, "employee", "boss").unwrap();
        assert!(!authorized_index.contains("classified"));
        assert!(
            store
                .read_chunk(&acl, "employee", "boss", "private", &private.chunk_id)
                .is_err()
        );
        assert!(
            store
                .search(&acl, "employee", "boss", "private", "classified")
                .is_err()
        );
        assert!(
            store
                .zoom(&acl, "employee", "boss", "private", "topic:inbox")
                .is_err()
        );
        assert!(
            store
                .surface_fallback(&acl, "employee", "boss", "private", 10)
                .is_err()
        );
        let src = t.join("legacy");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("one.md"), "durable fact").unwrap();
        assert_eq!(
            store
                .import_folder(&acl, "boss-user", "boss", src.to_str().unwrap(), "work")
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .import_folder(&acl, "boss-user", "boss", src.to_str().unwrap(), "work")
                .unwrap(),
            1
        );
        let mut paths = Vec::new();
        collect_chunks(&t.join("store/boss/collections/work"), &mut paths).unwrap();
        assert_eq!(paths.len(), 1);
        let path = t
            .join("store/boss/collections/work/topics/inbox")
            .join(format!("{}.md", paths[0].chunk_id));
        fs::write(path, "broken").unwrap();
        assert!(
            store
                .read_chunk(&acl, "employee", "boss", "work", &paths[0].chunk_id)
                .is_err()
        );
        assert_eq!(
            fs::read_dir(t.join("store/boss/quarantine"))
                .unwrap()
                .count(),
            1
        );
        let _ = fs::remove_dir_all(t);
    }
}
