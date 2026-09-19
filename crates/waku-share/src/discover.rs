//! LAN discovery for waku daemons. A [`DaemonBrowser`] binds a lightweight
//! iroh endpoint (no relay, no outbound traffic of its own) and listens for
//! the multicast announcements every `ShareNode` publishes through
//! `MdnsAddressLookup`. Each discovered endpoint is probed over
//! [`link::ALPN_WAKU_LINK`]; those that answer `Info` surface as
//! [`DiscoveredDaemon`]s with enough detail to connect or pair.

use std::collections::HashSet;
use std::net::IpAddr;

use anyhow::Context as _;
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId};
use iroh_mdns_address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use n0_future::StreamExt;

use crate::link::{self, DaemonInfo, PairOutcome};

/// A user-data marker every waku daemon publishes on its endpoint, so
/// browsers can skip non-waku iroh nodes without dialing them.
pub const WAKU_USER_DATA: &str = "waku-daemon";

/// How long to wait for an `Info` reply — a LAN peer answers in
/// milliseconds or not at all.
const INFO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// One change in the set of nearby daemons.
#[derive(Clone, Debug)]
pub enum DiscoveryUpdate {
    /// A waku endpoint answered `Info` — new or changed addresses.
    Found(DiscoveredDaemon),
    /// An endpoint stopped announcing itself.
    Gone(EndpointId),
}

/// A daemon discovered on the local network, ready to connect or pair.
#[derive(Clone, Debug)]
pub struct DiscoveredDaemon {
    /// The endpoint's stable identity — also its friend code.
    pub endpoint_id: EndpointId,
    /// Dialable address for `request_pair`.
    pub addr: EndpointAddr,
    /// What the endpoint reported about itself.
    pub info: DaemonInfo,
    /// `ws://host:port` built from the announced LAN addresses —
    /// `Some` only when the daemon is actually exposed (`info.ws_port`).
    pub ws_url: Option<String>,
}

/// Browses the LAN for waku daemons. Async because it owns an iroh
/// endpoint; hosts drive it on their own runtime (the desktop runs a
/// current-thread runtime on a dedicated thread, like the share service).
pub struct DaemonBrowser {
    endpoint: Endpoint,
    updates: tokio::sync::mpsc::UnboundedReceiver<DiscoveryUpdate>,
}

impl DaemonBrowser {
    /// Bind a Minimal endpoint and start draining mDNS discovery events.
    pub async fn spawn() -> anyhow::Result<Self> {
        let endpoint = Endpoint::bind(presets::Minimal)
            .await
            .context("binding discovery endpoint")?;
        let mdns = MdnsAddressLookup::builder()
            .build(endpoint.id())
            .context("building mdns address lookup")?;
        endpoint
            .address_lookup()
            .context("endpoint has no address lookup registry")?
            .add(mdns.clone());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(run(endpoint.clone(), mdns, tx));
        Ok(Self {
            endpoint,
            updates: rx,
        })
    }

    /// The next discovery change; `None` once the browser is shut down.
    pub async fn next(&mut self) -> Option<DiscoveryUpdate> {
        self.updates.recv().await
    }

    /// Ask a discovered daemon for a client token. Blocks until the remote
    /// user decides or `timeout` elapses.
    pub async fn request_pair(
        &self,
        daemon: &DiscoveredDaemon,
        device_name: &str,
        timeout: std::time::Duration,
    ) -> anyhow::Result<PairOutcome> {
        link::request_pair(&self.endpoint, daemon.addr.clone(), device_name, timeout).await
    }

    pub async fn shutdown(self) -> anyhow::Result<()> {
        self.endpoint.close().await;
        Ok(())
    }
}

async fn run(
    endpoint: Endpoint,
    mdns: MdnsAddressLookup,
    tx: tokio::sync::mpsc::UnboundedSender<DiscoveryUpdate>,
) {
    let mut events = mdns.subscribe().await;
    // Endpoints re-announce; only re-probe when the address set changed.
    let mut probed: HashSet<EndpointId> = HashSet::new();
    while let Some(event) = events.next().await {
        match event {
            DiscoveryEvent::Discovered { endpoint_info, .. } => {
                if !is_waku(&endpoint_info) || !probed.insert(endpoint_info.endpoint_id) {
                    continue;
                }
                let addr = endpoint_info.to_endpoint_addr();
                let endpoint = endpoint.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let Ok(info) =
                        tokio::time::timeout(INFO_TIMEOUT, link::fetch_info(&endpoint, addr.clone()))
                            .await
                            .map_err(|_| anyhow::anyhow!("info timed out"))
                            .and_then(|r| r)
                    else {
                        return;
                    };
                    let _ = tx.send(DiscoveryUpdate::Found(DiscoveredDaemon {
                        endpoint_id: addr.id,
                        ws_url: info.ws_port.map(|port| ws_url(&addr, port)),
                        addr,
                        info,
                    }));
                });
            }
            DiscoveryEvent::Expired { endpoint_id } => {
                probed.remove(&endpoint_id);
                let _ = tx.send(DiscoveryUpdate::Gone(endpoint_id));
            }
            _ => {}
        }
    }
}

fn is_waku(info: &iroh::address_lookup::EndpointInfo) -> bool {
    info.user_data()
        .is_some_and(|data| data.to_string() == WAKU_USER_DATA)
}

fn ws_url(addr: &EndpointAddr, port: u16) -> String {
    // Prefer the first public-ish IPv4; a LAN peer dials what it announced.
    let ip = addr
        .ip_addrs()
        .find(|addr| addr.ip().is_ipv4() && !addr.ip().is_loopback())
        .or_else(|| addr.ip_addrs().find(|addr| !addr.ip().is_loopback()))
        .map(|addr| addr.ip())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    format!("ws://{ip}:{port}")
}
