//! Daemon-owned client pairing. A device that can reach the daemon — over
//! the WebSocket listener or the share endpoint's `waku-link` ALPN — asks
//! for a token; the request publishes into `PairingState` until a
//! connected client approves it, at which point the daemon mints a
//! per-device token into `paired-clients.json` and hands it back. Minted
//! tokens authenticate like the main bearer until revoked.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, bail};
use crossbeam_channel::{Receiver, Sender, bounded};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use uuid::Uuid;
use waku_protocol::pairing::{PairRequestInfo, PairedClientInfo, PairingState};

/// Cap on unanswered pair requests — each parks a socket or stream.
const MAX_PENDING: usize = 8;
/// How long a request may wait for a human decision before it expires.
const PAIR_TIMEOUT: Duration = Duration::from_secs(90);

/// Installed by the server so `PairingChanged` reaches every subscriber.
pub type PairingSink = Arc<dyn Fn(PairingState) + Send + Sync>;

/// What a pair request resolved to, as the requesting transport sees it.
#[derive(Clone, Debug)]
pub enum PairReply {
    Granted {
        token: String,
    },
    Declined {
        message: String,
    },
    /// Never queued — the pending list is full.
    Busy {
        message: String,
    },
}

/// A pending request's terminal outcome — the link transport maps it onto
/// its own wire enum, so it has to be nameable outside this module.
pub enum PairDecision {
    Granted { token: String },
    Declined { message: String },
}

struct PendingPair {
    device_name: String,
    transport: String,
    at_ms: u64,
    reply: Sender<PairDecision>,
}

#[derive(Clone, Deserialize, Serialize)]
struct StoredClient {
    id: Uuid,
    name: String,
    token: String,
    added_at_ms: u64,
}

#[derive(Default, Deserialize, Serialize)]
struct PairingStore {
    clients: Vec<StoredClient>,
}

struct PairingInner {
    pending: HashMap<Uuid, PendingPair>,
    store: PairingStore,
}

/// Sync facade both transports call; the async share runtime wraps
/// `register` + the receiver in a oneshot instead of blocking.
pub struct PairingService {
    inner: Mutex<PairingInner>,
    sink: Mutex<Option<PairingSink>>,
    store_path: PathBuf,
    /// The name granted clients learn — the same `USER`-derived label the
    /// share service reports to friends.
    daemon_name: String,
}

impl PairingService {
    pub fn new(data_dir: &Path, daemon_name: String) -> Self {
        let store_path = data_dir.join("paired-clients.json");
        let store = std::fs::read(&store_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            inner: Mutex::new(PairingInner {
                pending: HashMap::new(),
                store,
            }),
            sink: Mutex::new(None),
            store_path,
            daemon_name,
        }
    }

    pub fn daemon_name(&self) -> &str {
        &self.daemon_name
    }

    /// Where the server installs the `PairingChanged` broadcast.
    pub fn set_sink(&self, sink: PairingSink) {
        *self.sink.lock() = Some(sink);
    }

    pub fn state(&self) -> PairingState {
        let inner = self.inner.lock();
        PairingState {
            pending: inner
                .pending
                .iter()
                .map(|(id, request)| PairRequestInfo {
                    request_id: *id,
                    device_name: request.device_name.clone(),
                    transport: request.transport.clone(),
                    at_ms: request.at_ms,
                })
                .collect(),
            clients: inner
                .store
                .clients
                .iter()
                .map(|client| PairedClientInfo {
                    client_id: client.id,
                    name: client.name.clone(),
                    added_at_ms: client.added_at_ms,
                })
                .collect(),
        }
    }

    /// Queue `device_name` for a local decision and return its id plus the
    /// receiver the eventual decision lands on. Publishes immediately so
    /// approving surfaces see the request.
    pub fn register(
        &self,
        device_name: &str,
        transport: &str,
    ) -> anyhow::Result<(Uuid, Receiver<PairDecision>)> {
        let device_name = sanitize_device_name(device_name);
        {
            let inner = self.inner.lock();
            if inner.pending.len() >= MAX_PENDING {
                bail!("too many pending pair requests; answer or wait out the open ones");
            }
        }
        let id = Uuid::new_v4();
        let (reply, rx) = bounded(1);
        self.inner.lock().pending.insert(
            id,
            PendingPair {
                device_name,
                transport: transport.to_string(),
                at_ms: now_ms(),
                reply,
            },
        );
        self.publish();
        Ok((id, rx))
    }

    /// The blocking path the WebSocket listener uses: register, then wait
    /// out the human decision. A timeout expires the pending entry so it
    /// can't be answered into a dead connection.
    pub fn request_blocking(&self, device_name: &str, transport: &str) -> PairReply {
        let (id, rx) = match self.register(device_name, transport) {
            Ok(pair) => pair,
            Err(error) => {
                return PairReply::Busy {
                    message: error.to_string(),
                };
            }
        };
        match rx.recv_timeout(PAIR_TIMEOUT) {
            Ok(PairDecision::Granted { token }) => PairReply::Granted { token },
            Ok(PairDecision::Declined { message }) => PairReply::Declined { message },
            Err(_) => {
                if self.inner.lock().pending.remove(&id).is_some() {
                    self.publish();
                }
                PairReply::Declined {
                    message: "pair request timed out".into(),
                }
            }
        }
    }

    /// Settle a pending request from an authenticated client. Accepting
    /// mints and persists a per-device token before replying.
    pub fn respond(&self, request_id: Uuid, accept: bool) -> anyhow::Result<()> {
        let pending = self.inner.lock().pending.remove(&request_id);
        let Some(pending) = pending else {
            bail!("no pending pair request with that id");
        };
        let decision = if accept {
            let token = Uuid::new_v4().simple().to_string();
            {
                let mut inner = self.inner.lock();
                inner.store.clients.push(StoredClient {
                    id: Uuid::new_v4(),
                    name: pending.device_name.clone(),
                    token: token.clone(),
                    added_at_ms: now_ms(),
                });
            }
            if let Err(error) = self.save() {
                eprintln!("could not persist paired clients: {error:#}");
            }
            PairDecision::Granted { token }
        } else {
            PairDecision::Declined {
                message: "pair request declined".into(),
            }
        };
        let _ = pending.reply.send(decision);
        self.publish();
        Ok(())
    }

    /// Whether `candidate` is a minted, unrevoked paired-client token.
    pub fn authenticate(&self, token: &str) -> bool {
        self.inner
            .lock()
            .store
            .clients
            .iter()
            .any(|client| bool::from(client.token.as_bytes().ct_eq(token.as_bytes())))
    }

    pub fn revoke(&self, client_id: Uuid) -> anyhow::Result<()> {
        {
            let mut inner = self.inner.lock();
            let before = inner.store.clients.len();
            inner.store.clients.retain(|client| client.id != client_id);
            if inner.store.clients.len() == before {
                bail!("no paired client with that id");
            }
        }
        if let Err(error) = self.save() {
            eprintln!("could not persist paired clients: {error:#}");
        }
        self.publish();
        Ok(())
    }

    fn save(&self) -> anyhow::Result<()> {
        let inner = self.inner.lock();
        let data = serde_json::to_vec_pretty(&inner.store)?;
        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = self.store_path.with_extension("json.tmp");
        std::fs::write(&temporary, data)?;
        std::fs::rename(&temporary, &self.store_path).context("saving paired clients")
    }

    fn publish(&self) {
        let state = self.state();
        if let Some(sink) = self.sink.lock().as_ref() {
            sink(state);
        }
    }
}

/// Device names arrive over the wire; keep them single-line and bounded
/// for the approval prompt.
fn sanitize_device_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(80)
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        "unknown device".to_string()
    } else {
        cleaned.to_string()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir() -> TempDir {
        let path = std::env::temp_dir().join(format!("waku-pairing-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn service() -> (TempDir, PairingService) {
        let dir = temp_dir();
        let service = PairingService::new(dir.path(), "testbox".into());
        (dir, service)
    }

    #[test]
    fn approve_mints_an_authenticating_token() {
        let (_dir, service) = service();
        let (id, rx) = service.register("phone", "ws").unwrap();
        service.respond(id, true).unwrap();
        let PairDecision::Granted { token } = rx.recv_timeout(Duration::from_secs(1)).unwrap()
        else {
            panic!("expected a grant");
        };
        assert!(service.authenticate(&token));
        assert_eq!(service.state().clients.len(), 1);
    }

    #[test]
    fn decline_leaves_no_client() {
        let (_dir, service) = service();
        let (id, rx) = service.register("phone", "ws").unwrap();
        service.respond(id, false).unwrap();
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(1)),
            Ok(PairDecision::Declined { .. })
        ));
        assert!(service.state().clients.is_empty());
    }

    #[test]
    fn revoke_stops_the_token() {
        let (_dir, service) = service();
        let (id, rx) = service.register("phone", "ws").unwrap();
        service.respond(id, true).unwrap();
        let PairDecision::Granted { token } = rx.recv().unwrap() else {
            panic!("expected a grant");
        };
        let client_id = service.state().clients[0].client_id;
        service.revoke(client_id).unwrap();
        assert!(!service.authenticate(&token));
    }

    #[test]
    fn granted_tokens_survive_a_reload() {
        let dir = temp_dir();
        let token = {
            let service = PairingService::new(dir.path(), "testbox".into());
            let (id, rx) = service.register("phone", "ws").unwrap();
            service.respond(id, true).unwrap();
            let PairDecision::Granted { token } = rx.recv().unwrap() else {
                panic!("expected a grant");
            };
            token
        };
        let reloaded = PairingService::new(dir.path(), "testbox".into());
        assert!(reloaded.authenticate(&token));
    }

    #[test]
    fn pending_cap_rejects_new_requests() {
        let (_dir, service) = service();
        for _ in 0..MAX_PENDING {
            service.register("phone", "ws").unwrap();
        }
        assert!(service.register("one more", "ws").is_err());
    }
}
