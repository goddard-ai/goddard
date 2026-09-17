//! P2P file sharing between Goddard instances.
//!
//! Sender side: [`Provider`] imports a path into an `iroh-blobs` store and
//! serves it over QUIC. Receiver side: [`fetch`] connects by ticket and
//! streams a verified copy out. Both follow sendme's wire protocol, so
//! transfers are interoperable with `sendme send`/`receive`.

pub mod friends;
pub mod identity;

use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use iroh::{Endpoint, EndpointAddr, RelayMode, endpoint::presets};
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
/// This is what crosses the "friend" channel inside our own offer message;
/// it also serializes to a sendme-compatible ticket string.
pub type Ticket = BlobTicket;

/// A running send: serves the imported blob until dropped/shutdown.
pub struct Provider {
    router: iroh::protocol::Router,
    store: FsStore,
    hash: Hash,
    _dir: PathBuf,
}

impl Provider {
    /// Serve `path` (file or directory) to anyone holding the returned ticket.
    /// `blobs_dir` is where the blob store keeps its data; it must be a
    /// Goddard-owned directory, never the cwd (sendme's `.sendme-*` wart).
    ///
    /// `relay` controls NAT traversal: `RelayMode::Default` uses n0's public
    /// relays + DNS discovery, `RelayMode::Disabled` restricts to local/LAN
    /// addressing (used by the spike and tests).
    pub async fn start(path: &Path, blobs_dir: &Path, relay: RelayMode) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(blobs_dir).await?;
        let endpoint = Endpoint::builder(presets::N0)
            .relay_mode(relay)
            .bind()
            .await
            .context("binding iroh endpoint")?;
        let store = FsStore::load(blobs_dir)
            .await
            .context("loading blob store")?;
        let blobs = BlobsProtocol::new(&store, None);

        let import = store.blobs().add_path_with_opts(AddPathOptions {
            path: path.to_path_buf(),
            mode: ImportMode::TryReference,
            format: BlobFormat::Raw,
        });
        let mut stream = import.stream().await;
        let mut hash = None;
        while let Some(item) = stream.next().await {
            match item {
                AddProgressItem::Error(cause) => bail!("importing {}: {cause}", path.display()),
                AddProgressItem::Done(tag) => hash = Some(tag.hash()),
                _ => {}
            }
        }
        let hash = hash.context("import finished without a hash")?;

        let router = iroh::protocol::Router::builder(endpoint)
            .accept(iroh_blobs::ALPN, blobs)
            .spawn();
        Ok(Self {
            router,
            store,
            hash,
            _dir: blobs_dir.to_path_buf(),
        })
    }

    /// Address + hash the receiver needs. Call after [`Self::wait_online`] if
    /// you want relay/discovery info included.
    pub fn ticket(&self) -> Ticket {
        // For the spike we import a single file as a Raw blob; the real
        // protocol will build a HashSeq collection so folders work too.
        BlobTicket::new(self.endpoint_addr(), self.hash, BlobFormat::Raw)
    }

    pub fn endpoint_addr(&self) -> EndpointAddr {
        self.router.endpoint().addr()
    }

    /// Wait for relay/discovery so the ticket contains a reachable address.
    pub async fn wait_online(&self) {
        let _ = self.router.endpoint().online().await;
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

/// Fetch the content behind `ticket` into `dest_dir` under `blobs_dir`'s store.
/// Streams verified chunks; returns the hash of what landed.
pub async fn fetch(ticket: &Ticket, blobs_dir: &Path, relay: RelayMode) -> anyhow::Result<Hash> {
    tokio::fs::create_dir_all(blobs_dir).await?;
    let endpoint = Endpoint::builder(presets::N0)
        .relay_mode(relay)
        .bind()
        .await
        .context("binding iroh endpoint")?;
    let store = FsStore::load(blobs_dir).await?;

    let result = fetch_inner(&endpoint, &store, ticket).await;

    endpoint.close().await;
    store.shutdown().await?;
    result
}

async fn fetch_inner(
    endpoint: &Endpoint,
    store: &Store,
    ticket: &Ticket,
) -> anyhow::Result<Hash> {
    let hash_and_format = ticket.hash_and_format();
    let connection = endpoint
        .connect(ticket.addr().clone(), iroh_blobs::protocol::ALPN)
        .await
        .context("connecting to sender")?;
    let local = store.remote().local(hash_and_format).await?;
    let get = store
        .remote()
        .execute_get(connection, local.missing());
    let mut stream = get.stream();
    while let Some(item) = stream.next().await {
        match item {
            GetProgressItem::Progress(_) => {}
            GetProgressItem::Done(_) => break,
            GetProgressItem::Error(cause) => bail!("download failed: {cause}"),
        }
    }
    Ok(hash_and_format.hash)
}

/// Write a fetched blob out of the store to `target` (streams, sparse-aware).
pub async fn export_blob(store_dir: &Path, hash: Hash, target: &Path) -> anyhow::Result<()> {
    let store = FsStore::load(store_dir).await?;
    let mut stream = store.blobs().export(hash, target).stream().await;
    while let Some(item) = stream.next().await {
        match item {
            ExportProgressItem::Error(cause) => bail!("exporting: {cause}"),
            ExportProgressItem::Done => break,
            _ => {}
        }
    }
    store.shutdown().await?;
    Ok(())
}
