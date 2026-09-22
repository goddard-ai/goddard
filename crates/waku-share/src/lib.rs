//! P2P file sharing between Goddard instances.
//!
//! One [`ShareNode`] per app: a single iroh endpoint that serves both the
//! friends control channel ([`friends::ALPN_FRIENDS`]) and the `iroh-blobs`
//! transfer protocol. `provide()` imports a path into the blob store and
//! returns a sendme-compatible ticket; `fetch_to()` dials a ticket and
//! streams a verified copy out.

pub mod discover;
pub mod friends;
pub mod identity;
pub mod link;
pub mod projects;

pub use iroh::{EndpointId, RelayMode};
pub use iroh_blobs::api::TempTag;

use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointAddr, SecretKey, endpoint::presets};
use iroh_blobs::{
    BlobFormat, BlobsProtocol, Hash, HashAndFormat,
    api::Store,
    api::blobs::{AddPathOptions, AddProgressItem, ExportProgressItem, ImportMode},
    api::remote::GetProgressItem,
    format::collection::Collection,
    store::fs::FsStore,
    ticket::BlobTicket,
};
use iroh_mdns_address_lookup::MdnsAddressLookup;
use n0_future::StreamExt;

/// The blake3 hash of the shared content plus the sender's dialable address.
/// Serializes to a sendme-compatible ticket string.
pub type Ticket = BlobTicket;

/// Bound on dialing the sender for a fetch — an unreachable peer must fail
/// the transfer instead of hanging it.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// One peer: endpoint + blob store + protocol router + LAN address lookup.
/// In the daemon this is a singleton; the spike runs two in one process.
pub struct ShareNode {
    router: Router,
    store: FsStore,
    /// Kept alive so the endpoint keeps announcing itself on the LAN;
    /// `subscribe()` feeds "nearby endpoints" surfaces.
    mdns: MdnsAddressLookup,
}

impl ShareNode {
    /// Bind an endpoint for `secret`, host `friends` on
    /// [`friends::ALPN_FRIENDS`], blobs on `iroh_blobs::ALPN`, and the
    /// daemon-link protocol on [`link::ALPN_WAKU_LINK`] when `link` is
    /// provided. `dir` holds the FsStore. `RelayMode::Default` uses n0's
    /// public relays and DNS discovery; `Disabled` is LAN-only (tests).
    /// Every node also announces itself via multicast so browsers and LAN
    /// friends resolve it without a relay.
    pub async fn spawn(
        dir: &Path,
        secret: SecretKey,
        relay: RelayMode,
        friends: friends::FriendsProtocol,
        link: Option<link::LinkProtocol>,
    ) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(dir.join("blobs")).await?;
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret)
            .relay_mode(relay)
            .user_data_for_address_lookup(
                discover::WAKU_USER_DATA
                    .parse()
                    .context("static user data marker is invalid")?,
            )
            .bind()
            .await
            .context("binding iroh endpoint")?;
        let mdns = MdnsAddressLookup::builder()
            .build(endpoint.id())
            .context("building mdns address lookup")?;
        endpoint
            .address_lookup()
            .context("endpoint has no address lookup registry")?
            .add(mdns.clone());
        let store = FsStore::load(dir.join("blobs")).await?;
        let blobs = BlobsProtocol::new(&store, None);
        let mut router = Router::builder(endpoint)
            .accept(friends::ALPN_FRIENDS, friends)
            .accept(iroh_blobs::ALPN, blobs);
        if let Some(link) = link {
            router = router.accept(link::ALPN_WAKU_LINK, link);
        }
        let router = router.spawn();
        Ok(Self {
            router,
            store,
            mdns,
        })
    }

    pub fn endpoint(&self) -> &Endpoint {
        self.router.endpoint()
    }

    pub fn addr(&self) -> EndpointAddr {
        self.router.endpoint().addr()
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Wait for relay/discovery so tickets carry a reachable address.
    pub async fn wait_online(&self) {
        let _ = self.router.endpoint().online().await;
    }

    /// Stream of LAN discovery events — which endpoints are announcing
    /// themselves nearby right now.
    pub async fn nearby(&self) -> impl StreamExt<Item = iroh_mdns_address_lookup::DiscoveryEvent> {
        self.mdns.subscribe().await
    }

    /// Import `path` and return the ticket, the temp tag that keeps the
    /// content pinned against GC, and the declared payload size in bytes.
    /// Drop the tag to unpin (e.g. transfer cancelled). Directories import
    /// as a HashSeq collection of their files, so the ticket's format tells
    /// the receiver what shape to expect.
    pub async fn provide(&self, path: &Path) -> anyhow::Result<(Ticket, TempTag, u64)> {
        if path.is_dir() {
            return self.provide_dir(path).await;
        }
        let (tag, size) = self.import_file(path).await?;
        Ok((
            BlobTicket::new(self.addr(), tag.hash(), BlobFormat::Raw),
            tag,
            size,
        ))
    }

    /// Single file → raw blob. Returns the pin tag and the byte size the
    /// store reported while importing.
    async fn import_file(&self, path: &Path) -> anyhow::Result<(TempTag, u64)> {
        let import = self.store.blobs().add_path_with_opts(AddPathOptions {
            path: path.to_path_buf(),
            mode: ImportMode::TryReference,
            format: BlobFormat::Raw,
        });
        let mut stream = import.stream().await;
        let mut tag = None;
        let mut size = 0;
        while let Some(item) = stream.next().await {
            match item {
                AddProgressItem::Size(n) => size = n,
                AddProgressItem::Error(cause) => bail!("importing {}: {cause}", path.display()),
                AddProgressItem::Done(t) => tag = Some(t),
                _ => {}
            }
        }
        let tag = tag.context("import finished without a hash")?;
        Ok((tag, size))
    }

    /// Folder → `Collection` of its files → HashSeq root. Only regular
    /// files transfer; symlinks and special files are skipped so the tree
    /// can't escape the picked folder.
    async fn provide_dir(&self, dir: &Path) -> anyhow::Result<(Ticket, TempTag, u64)> {
        let mut files = Vec::new();
        collect_files(dir, dir, &mut files)
            .with_context(|| format!("reading {}", dir.display()))?;
        files.sort();
        let mut collection = Collection::default();
        let mut size = 0u64;
        // Child tags stay alive until the collection's root tag exists —
        // from then on the HashSeq tag pins the whole tree.
        let mut child_tags = Vec::with_capacity(files.len());
        for (name, file) in files {
            let (tag, n) = self.import_file(&file).await?;
            collection.push(name, tag.hash());
            child_tags.push(tag);
            size += n;
        }
        let tag = collection
            .store(self.store())
            .await
            .map_err(|e| anyhow::anyhow!("storing collection: {e:?}"))?;
        drop(child_tags);
        Ok((
            BlobTicket::new(self.addr(), tag.hash(), BlobFormat::HashSeq),
            tag,
            size,
        ))
    }

    /// Fetch `ticket`'s content into this node's store. Verified chunks;
    /// returns the hash. Resumable — a second call picks up missing ranges.
    /// `on_progress` receives payload bytes downloaded so far.
    pub async fn fetch(
        &self,
        ticket: &Ticket,
        mut on_progress: impl FnMut(u64),
    ) -> anyhow::Result<Hash> {
        let hash_and_format = ticket.hash_and_format();
        let connection = tokio::time::timeout(
            CONNECT_TIMEOUT,
            self.endpoint()
                .connect(ticket.addr().clone(), iroh_blobs::protocol::ALPN),
        )
        .await
        .map_err(|_| anyhow::anyhow!("timed out connecting to sender"))?
        .context("connecting to sender")?;
        let local = self.store.remote().local(hash_and_format).await?;
        let get = self.store.remote().execute_get(connection, local.missing());
        let mut stream = get.stream();
        let mut completed = false;
        while let Some(item) = stream.next().await {
            match item {
                GetProgressItem::Progress(done) => on_progress(done),
                GetProgressItem::Done(stats) => {
                    on_progress(stats.payload_bytes_read);
                    completed = true;
                    break;
                }
                GetProgressItem::Error(cause) => bail!("download failed: {cause}"),
            }
        }
        if !completed {
            bail!("download stream ended before completion");
        }
        Ok(hash_and_format.hash)
    }

    /// Write fetched content out of the store: a raw blob becomes the
    /// `target` file, a HashSeq collection becomes the `target` directory
    /// tree.
    pub async fn export(&self, content: HashAndFormat, target: &Path) -> anyhow::Result<()> {
        match content.format {
            BlobFormat::HashSeq => self.export_collection(content.hash, target).await,
            BlobFormat::Raw => self.export_blob(content.hash, target).await,
        }
    }

    /// Write a fetched blob out of the store to `target` (streams,
    /// sparse-aware).
    async fn export_blob(&self, hash: Hash, target: &Path) -> anyhow::Result<()> {
        let mut stream = self.store.blobs().export(hash, target).stream().await;
        let mut completed = false;
        while let Some(item) = stream.next().await {
            match item {
                ExportProgressItem::Error(cause) => bail!("exporting: {cause}"),
                ExportProgressItem::Done => {
                    completed = true;
                    break;
                }
                _ => {}
            }
        }
        if !completed {
            bail!("export stream ended before completion");
        }
        Ok(())
    }

    /// Rebuild the folder tree a HashSeq collection describes. Entry names
    /// arrive over the wire, so each path component is sanitized before it
    /// touches the filesystem.
    async fn export_collection(&self, root: Hash, target: &Path) -> anyhow::Result<()> {
        let collection = Collection::load(root, self.store())
            .await
            .map_err(|e| anyhow::anyhow!("loading collection: {e:?}"))?;
        std::fs::create_dir_all(target)?;
        let mut used = std::collections::HashSet::new();
        for (name, hash) in collection.iter() {
            let dest = target.join(collection_entry_path(name, &mut used));
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            self.export_blob(*hash, &dest).await?;
        }
        Ok(())
    }

    pub async fn shutdown(self) -> anyhow::Result<()> {
        // Store first: the router's blobs protocol holds a handle to it, so
        // shutting the router down first would close the store's RPC channel
        // from under this call.
        self.store.shutdown().await?;
        self.router.shutdown().await?;
        Ok(())
    }
}

/// Recursive regular-file listing under `dir`; names are relative to `root`
/// with `/` separators (the collection's wire naming). `file_type` does not
/// follow symlinks, so links and special files are skipped — the walk can
/// neither escape the picked folder nor cycle.
fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_files(root, &path, out)?;
        } else if file_type.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.push((rel, path));
        }
    }
    Ok(())
}

/// One wire-supplied collection path component → a safe file name, or None
/// when nothing usable remains. Separators and control characters become
/// `-`; the bare `.`/`..` components that would escape `target` drop out.
/// Leading dots are otherwise kept — dotfiles are legitimate payload.
fn sanitize_component(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return None;
    }
    // 255-byte filesystem component limit.
    let mut out = String::new();
    for c in cleaned.chars() {
        if out.len() + c.len_utf8() > 255 {
            break;
        }
        out.push(c);
    }
    let out = out.trim_end().to_string();
    if out.is_empty() { None } else { Some(out) }
}

/// Wire-supplied collection entry name → a safe relative path under the
/// export target. `used` tracks already-claimed destinations so sanitizing
/// can't silently overwrite an earlier entry; collisions get a `-n`
/// suffix on the file name.
fn collection_entry_path(
    raw: &str,
    used: &mut std::collections::HashSet<PathBuf>,
) -> PathBuf {
    let mut rel = PathBuf::new();
    for part in raw.split('/') {
        if let Some(part) = sanitize_component(part) {
            rel.push(part);
        }
    }
    if rel.as_os_str().is_empty() {
        rel.push("file");
    }
    if used.insert(rel.clone()) {
        return rel;
    }
    for n in 2u32.. {
        let mut candidate = rel.clone();
        let leaf = rel
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        candidate.set_file_name(format!("{leaf}-{n}"));
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}
