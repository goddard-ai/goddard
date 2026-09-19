//! LAN daemon discovery for the desktop app. Wraps `waku-share`'s
//! `DaemonBrowser` — a Minimal iroh endpoint plus `MdnsAddressLookup` — in
//! the crate's sync style: one dedicated thread runs the runtime, updates
//! arrive on a crossbeam channel. Pair requests go over the encrypted
//! `waku-link` ALPN, so the granted token never crosses the LAN in
//! cleartext.

use std::time::Duration;

use anyhow::Context as _;
use crossbeam_channel::{Receiver, Sender};

pub use waku_share::discover::{DiscoveredDaemon, DiscoveryUpdate};
pub use waku_share::link::PairOutcome;

/// How long a granted-or-declined answer may take — the daemon's own
/// pending timeout is shorter, so this bounds only transport failure.
const PAIR_TIMEOUT: Duration = Duration::from_secs(100);

/// Handle for a running discovery thread. `updates` yields `Found`/`Gone`
/// changes as waku daemons announce and expire on the LAN.
pub struct DaemonDiscovery {
    /// Discovered/expired daemons. Ends when the browser task stops.
    pub updates: Receiver<DiscoveryUpdate>,
    pair_requests: Sender<PairRequest>,
    _thread: std::thread::JoinHandle<()>,
}

struct PairRequest {
    daemon: DiscoveredDaemon,
    device_name: String,
    reply: Sender<anyhow::Result<PairOutcome>>,
}

impl DaemonDiscovery {
    /// Bind the discovery endpoint and start draining LAN announcements.
    /// Returns promptly; the endpoint binds inside the worker thread.
    pub fn start() -> Self {
        let (updates_tx, updates_rx) = crossbeam_channel::unbounded();
        let (pair_tx, pair_rx) = crossbeam_channel::unbounded::<PairRequest>();
        let thread = std::thread::Builder::new()
            .name("goddard-discovery".into())
            .spawn(move || {
                if let Err(error) = run(updates_tx, pair_rx) {
                    eprintln!("daemon discovery stopped: {error:#}");
                }
            })
            .expect("could not start daemon discovery thread");
        Self {
            updates: updates_rx,
            pair_requests: pair_tx,
            _thread: thread,
        }
    }

    /// Ask a discovered daemon for a client token. Blocks until the
    /// remote user decides, so callers run it off the UI thread.
    pub fn request_pair(
        &self,
        daemon: &DiscoveredDaemon,
        device_name: &str,
    ) -> anyhow::Result<PairOutcome> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.pair_requests
            .send(PairRequest {
                daemon: daemon.clone(),
                device_name: device_name.to_string(),
                reply: tx,
            })
            .context("daemon discovery is not running")?;
        rx.recv_timeout(PAIR_TIMEOUT)
            .context("pair request timed out")?
    }
}

fn run(
    updates: Sender<DiscoveryUpdate>,
    pair_requests: Receiver<PairRequest>,
) -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let mut browser = waku_share::discover::DaemonBrowser::spawn().await?;
        loop {
            tokio::select! {
                update = browser.next() => {
                    let Some(update) = update else { return anyhow::Ok(()); };
                    if updates.send(update).is_err() {
                        return anyhow::Ok(());
                    }
                }
                request = async {
                    // crossbeam recv is blocking; park it on a blocking
                    // thread so the runtime keeps draining announcements
                    // while a pair request waits for a human decision.
                    tokio::task::spawn_blocking({
                        let rx = pair_requests.clone();
                        move || rx.recv()
                    }).await
                } => {
                    match request {
                        Ok(Ok(request)) => {
                            let browser_daemon = request.daemon.clone();
                            let outcome = browser
                                .request_pair(
                                    &browser_daemon,
                                    &request.device_name,
                                    PAIR_TIMEOUT,
                                )
                                .await;
                            let _ = request.reply.send(outcome);
                        }
                        _ => return anyhow::Ok(()),
                    }
                }
            }
        }
    })
}
