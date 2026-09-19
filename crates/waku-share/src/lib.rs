//! P2P file sharing between Goddard instances.
//!
//! One [`ShareNode`] per app: a single iroh endpoint that serves both the
//! friends control channel ([`friends::ALPN_FRIENDS`]) and the `iroh-blobs`
//! transfer protocol. `provide()` imports a path into the blob store and
//! returns a sendme-compatible ticket; `fetch_to()` dials a ticket and
//! streams a verified copy out.

pub mod friends;
pub mod identity;
pub mod projects;

pub use iroh::{EndpointId, RelayMode};
pub use iroh_blobs::api::TempTag;

use std::path::Path;

use anyhow::{Context as _, bail};
use iroh::{Endpoint, EndpointAddr, SecretKey, endpoint::presets};
use iroh::protocol::Router;
use iroh_blobs::{
    BlobFormat, BlobsProtocol, Hash,
    api::Store,
    api::blobs::{AddPathOptions, AddProgressItem, ExportProgressItem, ImportMode},
    api::remote::GetProgressItem,
    store::fs::FsStore,
    ticket::BlobTicket,
};
use n0_future::StreamExt;

/// The blake3 hash of the shared content plus the sender's dialable address.
/// Serializes to a sendme-compatible ticket string.
pub type Ticket = BlobTicket;

/// Bound on dialing the sender for a fetch — an unreachable peer must fail
/// the transfer instead of hanging it.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// One peer: endpoint + blob store + protocol router. In the daemon this is
/// a singleton; the spike runs two in one process.
pub struct ShareNode {
    router: Router,
    store: FsStore,
}

impl ShareNode {
    /// Bind an endpoint for `secret`, host `friends` on
    /// [`friends::ALPN_FRIENDS`] and blobs on `iroh_blobs::ALPN`.
    /// `dir` holds the FsStore. `RelayMode::Default` uses n0's public relays
    /// and DNS discovery; `Disabled` is LAN-only (tests).
    pub async fn spawn(
        dir: &Path,
        secret: SecretKey,
        relay: RelayMode,
        friends: friends::FriendsProtocol,
    ) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(dir.join("blobs")).await?;
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret)
            .relay_mode(relay)
            .bind()
            .await
            .context("binding iroh endpoint")?;
        let store = FsStore::load(dir.join("blobs")).await?;
        let blobs = BlobsProtocol::new(&store, None);
        let router = Router::builder(endpoint)
            .accept(friends::ALPN_FRIENDS, friends)
            .accept(iroh_blobs::ALPN, blobs)
            .spawn();
        Ok(Self { router, store })
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

    /// Import `path` and return the ticket plus the temp tag that keeps it
    /// pinned against GC. Drop the tag to unpin (e.g. transfer cancelled).
    pub async fn provide(&self, path: &Path) -> anyhow::Result<(Ticket, TempTag)> {
        let import = self.store.blobs().add_path_with_opts(AddPathOptions {
            path: path.to_path_buf(),
            mode: ImportMode::TryReference,
            format: BlobFormat::Raw,
        });
        let mut stream = import.stream().await;
        let mut tag = None;
        while let Some(item) = stream.next().await {
            match item {
                AddProgressItem::Error(cause) => bail!("importing {}: {cause}", path.display()),
                AddProgressItem::Done(t) => tag = Some(t),
                _ => {}
            }
        }
        let tag = tag.context("import finished without a hash")?;
        // Raw blob for the spike; the real protocol builds a HashSeq
        // collection so folders work too.
        Ok((
            BlobTicket::new(self.addr(), tag.hash(), BlobFormat::Raw),
            tag,
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

    /// Write a fetched blob out of the store to `target` (streams,
    /// sparse-aware).
    pub async fn export(&self, hash: Hash, target: &Path) -> anyhow::Result<()> {
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

    pub async fn shutdown(self) -> anyhow::Result<()> {
        // Store first: the router's blobs protocol holds a handle to it, so
        // shutting the router down first would close the store's RPC channel
        // from under this call.
        self.store.shutdown().await?;
        self.router.shutdown().await?;
        Ok(())
    }
}
