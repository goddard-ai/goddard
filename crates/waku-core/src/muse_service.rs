//! A Goddard-owned `muse serve` host shared by every Muse session.
//!
//! MSP is NDJSON JSON-RPC over the stdio of one long-lived `muse serve`
//! process. That host owns sessions for every workspace — `session/start`
//! carries `workspaceRoot` — so all Goddard tasks multiplex over a single
//! child instead of paying a process launch and an `initialize` handshake per
//! session. The pool keeps one host per resolved binary and kills it when the
//! last session handle drops.
//!
//! Demultiplexing happens here, not in the driver: every view notification
//! names its `sessionId` (the one exception, the `session/started` broadcast,
//! nests it under `session.sessionId`), and server-initiated
//! `approval/request` / `userInput/request` frames are acknowledged with `{}`
//! — "a client is handling this" — and republished so the owning session's
//! driver renders the prompt and later answers through `approval/decide` /
//! `userInput/answer`. Any other server-initiated request gets a typed
//! `methodNotFound` rather than a synthetic success.
//!
//! Everything here blocks — process spawn, pipe writes, request waits — so
//! callers must already be off the UI thread. Driver start and the daemon's
//! request threads are.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read as _, Write};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex as StdMutex, OnceLock, Weak};
use std::thread;
use std::time::Duration;

use anyhow::{Context as _, anyhow};
use crossbeam_channel::{Receiver, Sender, unbounded};
use serde_json::{Value, json};
use uuid::Uuid;

/// A command is admission-acknowledged quickly on a healthy host; the budget
/// covers a cold session load, not a turn.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
/// How long the last drop waits for `muse serve` to exit before SIGKILL.
const HOST_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
/// The largest line the reader accepts before treating the host as broken.
const MAX_LINE_BYTES: usize = 64 * 1024 * 1024;

/// What a subscribed session receives.
#[derive(Clone, Debug)]
pub(crate) enum MuseFrame {
    /// One live notification or acknowledged server-initiated request, as
    /// `{method, params}`.
    Event { method: String, params: Value },
    /// The host process is gone. Every session on it is over; the pool starts
    /// a fresh host for the NEXT session.
    Exited,
}

/// A failed `muse serve` call: either a wire error object or a local
/// transport failure.
#[derive(Debug)]
pub(crate) enum MuseError {
    /// The host's JSON-RPC error object (`code`, `message`, optional `data`).
    Rpc(Value),
    /// The call never completed — write failure, timeout, or a dead host.
    Transport(String),
}

impl MuseError {
    pub fn message(&self) -> String {
        match self {
            Self::Rpc(error) => error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown Muse host error")
                .to_owned(),
            Self::Transport(message) => message.clone(),
        }
    }

    /// MSP has no stable auth error category, so a login failure is
    /// recognized by the host saying so, never by an HTTP status or a guess.
    pub fn is_auth_failure(&self) -> bool {
        let message = self.message().to_lowercase();
        message.contains("not authenticated")
            || message.contains("not logged in")
            || message.contains("log in")
            || message.contains("login required")
            || message.contains("credential")
                && (message.contains("expired") || message.contains("invalid"))
    }
}

impl std::fmt::Display for MuseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

/// The shared host. All of a session's commands go through [`Self::call`];
/// its events arrive on a [`MuseSubscription`].
pub(crate) struct MuseHost {
    child: StdMutex<Child>,
    /// `None` once shutdown closes the pipe — the EOF the host exits on.
    writer: StdMutex<Option<ChildStdin>>,
    pending: StdMutex<HashMap<u64, Sender<Result<Value, MuseError>>>>,
    next_request_id: AtomicU64,
    hub: StdMutex<HashMap<String, Vec<(usize, Sender<MuseFrame>)>>>,
    next_subscriber: AtomicU64,
    alive: AtomicBool,
}

/// The pool handle whose last drop kills the host.
#[derive(Clone)]
pub(crate) struct MuseService {
    inner: Arc<MuseHost>,
    slot: Option<Weak<PoolSlot>>,
}

impl Deref for MuseService {
    type Target = MuseHost;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl MuseService {
    /// Every `method`+`params` frame the host sends for `session_id`, until
    /// the returned subscription drops.
    ///
    /// SUBSCRIBE BEFORE the lifecycle call that opens the session: the host
    /// auto-subscribes this connection at `session/start` / `session/resume`
    /// and can emit that session's first event before its response lands.
    pub(crate) fn subscribe(&self, session_id: &str) -> MuseSubscription {
        let (tx, rx) = unbounded();
        let subscriber_id = self.inner.next_subscriber.fetch_add(1, Ordering::Relaxed) as usize;
        self.inner
            .hub
            .lock()
            .unwrap()
            .entry(session_id.to_owned())
            .or_default()
            .push((subscriber_id, tx));
        MuseSubscription {
            host: Arc::downgrade(&self.inner),
            session_id: session_id.to_owned(),
            subscriber_id,
            rx,
        }
    }
}

impl Drop for MuseService {
    fn drop(&mut self) {
        let Some(slot) = self.slot.as_ref().and_then(Weak::upgrade) else {
            if Arc::strong_count(&self.inner) == 1 {
                self.inner.shutdown(HOST_EXIT_TIMEOUT);
            }
            return;
        };
        // The last strong handle wins teardown, decided under the slot lock:
        // `acquire`/`attached` upgrade the running Weak while holding it, so
        // the count check cannot race a new handle into existence.
        let mut state = slot.state.lock().unwrap();
        if Arc::strong_count(&self.inner) > 1 {
            return;
        }
        // Only the generation the slot still names may move it to Stopping —
        // a stale handle of a superseded dead host must shut its own process
        // down without clobbering the replacement's `Running` state.
        let mine = matches!(&*state, PoolState::Running(host) if host.ptr_eq(&Arc::downgrade(&self.inner)));
        if !mine {
            drop(state);
            self.inner.shutdown(HOST_EXIT_TIMEOUT);
            return;
        }
        *state = PoolState::Stopping;
        drop(state);
        self.inner.shutdown(HOST_EXIT_TIMEOUT);
        let mut state = slot.state.lock().unwrap();
        *state = PoolState::Vacant;
        slot.changed.notify_all();
    }
}

enum PoolState {
    Vacant,
    Starting,
    Running(Weak<MuseHost>),
    Stopping,
}

struct PoolSlot {
    state: StdMutex<PoolState>,
    changed: Condvar,
}

impl Default for PoolSlot {
    fn default() -> Self {
        Self {
            state: StdMutex::new(PoolState::Vacant),
            changed: Condvar::new(),
        }
    }
}

fn pool() -> &'static StdMutex<HashMap<PathBuf, Arc<PoolSlot>>> {
    static POOL: OnceLock<StdMutex<HashMap<PathBuf, Arc<PoolSlot>>>> = OnceLock::new();
    POOL.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Returns the host serving `binary`, starting one if none is alive.
///
/// Blocking (process start plus the `initialize` handshake), so callers must
/// already be off the UI thread.
pub(crate) fn acquire(binary: &Path) -> anyhow::Result<MuseService> {
    let slot = {
        let mut pool = pool().lock().unwrap();
        Arc::clone(pool.entry(binary.to_path_buf()).or_default())
    };

    let superseded = loop {
        let mut state = slot.state.lock().unwrap();
        if let PoolState::Running(host) = &*state {
            if let Some(host) = host.upgrade() {
                if host.is_alive() {
                    return Ok(MuseService {
                        inner: host,
                        slot: Some(Arc::downgrade(&slot)),
                    });
                }
                // The process exited while older handles still exist. Replace
                // this generation; their drops compare weak identity and so
                // cannot tear down the replacement.
                *state = PoolState::Starting;
                break Some(host);
            }
            // Strong count reached zero and a MuseService::drop is mid-teardown.
            state = slot.changed.wait(state).unwrap();
            drop(state);
            continue;
        }
        match &*state {
            PoolState::Vacant => {
                *state = PoolState::Starting;
                break None;
            }
            PoolState::Starting | PoolState::Stopping => {
                state = slot.changed.wait(state).unwrap();
                drop(state);
            }
            PoolState::Running(_) => unreachable!(),
        }
    };
    drop(superseded);

    let started = MuseHost::spawn(binary);
    let mut state = slot.state.lock().unwrap();
    match started {
        Ok(host) => {
            *state = PoolState::Running(Arc::downgrade(&host));
            slot.changed.notify_all();
            Ok(MuseService {
                inner: host,
                slot: Some(Arc::downgrade(&slot)),
            })
        }
        Err(error) => {
            *state = PoolState::Vacant;
            slot.changed.notify_all();
            Err(error)
        }
    }
}

/// A live host for `binary`, if the pool already has one.
///
/// Read-only probes — the session catalog, `model/list` — reuse a running
/// host rather than paying a spawn; `None` never starts one.
pub(crate) fn attached(binary: &Path) -> Option<MuseService> {
    let slot = pool().lock().unwrap().get(binary).cloned()?;
    let host = {
        let state = slot.state.lock().unwrap();
        match &*state {
            PoolState::Running(host) => host.upgrade(),
            _ => None,
        }
    }?;
    host.is_alive().then(|| MuseService {
        inner: host,
        slot: Some(Arc::downgrade(&slot)),
    })
}

impl MuseHost {
    fn spawn(binary: &Path) -> anyhow::Result<Arc<Self>> {
        let mut command = crate::command_env::command(binary);
        command.arg("serve");
        // A host serves every workspace, so its own cwd must not be a project
        // it could index or lock; an empty scratch directory keeps it neutral.
        let scratch = std::env::temp_dir().join(format!("waku-muse-serve-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).ok();
        let mut command = crate::command_env::guard_command(command);
        let mut child = crate::command_env::spawn(
            command
                .current_dir(&scratch)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null()),
        )
        .with_context(|| {
            format!(
                "failed to start `{} serve` — install Muse Code and run `muse login` first",
                binary.display()
            )
        })?;
        let (stdin, stdout) = match (child.stdin.take(), child.stdout.take()) {
            (Some(stdin), Some(stdout)) => (stdin, stdout),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(anyhow!("muse serve stdio pipes unavailable"));
            }
        };

        let host = Arc::new(Self {
            child: StdMutex::new(child),
            writer: StdMutex::new(Some(stdin)),
            pending: StdMutex::new(HashMap::new()),
            next_request_id: AtomicU64::new(1),
            hub: StdMutex::new(HashMap::new()),
            next_subscriber: AtomicU64::new(1),
            alive: AtomicBool::new(true),
        });
        // The reader starts before `initialize` so the handshake rides the
        // same request path — and its command timeout — as every later call.
        // A hung or non-MSP binary cannot block spawn forever, and every
        // failure below kills the child instead of leaking it.
        let reader_host = Arc::downgrade(&host);
        if let Err(error) = thread::Builder::new()
            .name("waku-muse-reader".into())
            .spawn(move || reader_loop(reader_host, BufReader::new(stdout)))
        {
            host.shutdown(Duration::ZERO);
            return Err(error.into());
        }
        let result = match host.call(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "waku",
                    "title": "Goddard",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        ) {
            Ok(result) => result,
            Err(error) => {
                host.shutdown(Duration::ZERO);
                return Err(anyhow!("muse serve did not initialize: {}", error.message()));
            }
        };

        let server_version = result
            .pointer("/serverInfo/version")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let fingerprint = result
            .pointer("/schema/fingerprint")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let durability = result
            .get("sessionDurability")
            .and_then(Value::as_str)
            .unwrap_or("durable");
        log(&format!(
            "muse serve ready: version={server_version} fingerprint={} durability={durability}",
            fingerprint.as_deref().unwrap_or("absent"),
        ));
        if let Err(error) = host.notify("initialized", json!({})) {
            host.shutdown(Duration::ZERO);
            return Err(error.into());
        }
        Ok(host)
    }

    /// Whether the reader still sees a live host. A dead host answers no
    /// further commands; the pool replaces it for the next `acquire`.
    pub(crate) fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// UUIDv7, the only command-id shape the host accepts.
    pub(crate) fn mint_command_id(&self) -> String {
        Uuid::now_v7().to_string()
    }

    /// One call to the host, blocking the calling thread until the response
    /// arrives, the timeout elapses, or the host dies. NOT for the UI thread.
    pub(crate) fn call(&self, method: &str, params: Value) -> Result<Value, MuseError> {
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = unbounded();
        self.pending.lock().unwrap().insert(id, tx);
        let frame = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        if let Err(error) = self.send(&frame) {
            self.pending.lock().unwrap().remove(&id);
            return Err(MuseError::Transport(format!(
                "muse serve write failed: {error}"
            )));
        }
        match rx.recv_timeout(COMMAND_TIMEOUT) {
            Ok(result) => result,
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                if !self.is_alive() {
                    Err(MuseError::Transport("muse serve exited".into()))
                } else {
                    Err(MuseError::Transport(format!(
                        "muse serve did not answer {method} within {}s",
                        COMMAND_TIMEOUT.as_secs()
                    )))
                }
            }
        }
    }

    /// Client-to-server notification; no response follows.
    pub(crate) fn notify(&self, method: &str, params: Value) -> Result<(), std::io::Error> {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn send(&self, frame: &Value) -> Result<(), std::io::Error> {
        let line = serde_json::to_string(frame)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        let mut writer = self.writer.lock().unwrap();
        let Some(writer) = writer.as_mut() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "muse serve stdin is closed",
            ));
        };
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()
    }

    /// Route one decoded frame to its pending request or its session.
    fn route(&self, frame: &Value) {
        // Response to one of our requests.
        if frame.get("method").is_none()
            && let Some(id) = frame.get("id").and_then(Value::as_u64)
        {
            if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
                if let Some(error) = frame.get("error") {
                    let _ = tx.send(Err(MuseError::Rpc(error.clone())));
                } else {
                    let _ = tx.send(Ok(frame.get("result").cloned().unwrap_or(Value::Null)));
                }
            }
            return;
        }

        let Some(method) = frame.get("method").and_then(Value::as_str) else {
            return;
        };
        let params = frame.get("params").cloned().unwrap_or(Value::Null);

        // A server-initiated REQUEST carries an id and expects a reply. The
        // two we handle are acknowledged so the host knows a client is on
        // them; the verdict travels as a separate `approval/decide` /
        // `userInput/answer` command. Anything else gets the typed
        // `methodNotFound` a synthetic `{}` would lie about.
        if let Some(id) = frame.get("id").cloned() {
            if matches!(method, "approval/request" | "userInput/request") {
                self.reply(id, Ok(json!({})));
            } else {
                self.reply(
                    id,
                    Err(json!({
                        "code": -32601,
                        "message": format!("method not found: {method}"),
                        "data": {"kind": "methodNotFound", "retryable": false},
                    })),
                );
                return;
            }
        }

        let session_id = if method == "session/started" {
            params
                .get("session")
                .and_then(|session| session.get("sessionId"))
                .and_then(Value::as_str)
        } else {
            params.get("sessionId").and_then(Value::as_str)
        };
        let Some(session_id) = session_id.map(str::to_owned) else {
            return;
        };
        self.publish(
            &session_id,
            MuseFrame::Event {
                method: method.to_owned(),
                params,
            },
        );
    }

    fn reply(&self, id: Value, result: Result<Value, Value>) {
        let frame = match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
        };
        let _ = self.send(&frame);
    }

    fn publish(&self, session_id: &str, frame: MuseFrame) {
        let Some(subscribers) = self.hub.lock().unwrap().get(session_id).cloned() else {
            return;
        };
        for (_, tx) in subscribers {
            let _ = tx.send(frame.clone());
        }
    }

    /// The host died or its stdout ended: every pending call and every
    /// session gets told exactly once.
    fn terminate(&self) {
        self.alive.store(false, Ordering::Relaxed);
        for (_, tx) in self.pending.lock().unwrap().drain() {
            let _ = tx.send(Err(MuseError::Transport(
                "muse serve exited mid-request".into(),
            )));
        }
        let mut hub = self.hub.lock().unwrap();
        for (_, subscribers) in hub.drain() {
            for (_, tx) in subscribers {
                let _ = tx.send(MuseFrame::Exited);
            }
        }
    }

    /// Close stdin, then wait briefly for the host to exit before killing it.
    /// Owned hosts only — the pool never runs this against a foreign process.
    fn shutdown(&self, budget: Duration) {
        self.alive.store(false, Ordering::Relaxed);
        // Take the pipe out of the mutex — dropping the guard alone leaves
        // stdin open and the host never sees the EOF that makes it exit.
        self.writer.lock().unwrap().take();
        let deadline = std::time::Instant::now() + budget;
        loop {
            let mut child = self.child.lock().unwrap();
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if std::time::Instant::now() < deadline => {
                    drop(child);
                    thread::sleep(Duration::from_millis(10));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
            }
        }
    }
}

/// A session's event stream; dropping it detaches the session from the hub.
pub(crate) struct MuseSubscription {
    host: Weak<MuseHost>,
    session_id: String,
    subscriber_id: usize,
    pub rx: Receiver<MuseFrame>,
}

impl MuseSubscription {
    /// An orphan subscription with no host: the channel never produces, and
    /// drop cannot reach a hub. Tests use it to build a `WorkerState`.
    #[cfg(test)]
    pub(crate) fn detached() -> Self {
        let (_tx, rx) = unbounded();
        Self {
            host: Weak::new(),
            session_id: String::new(),
            subscriber_id: 0,
            rx,
        }
    }
}

impl Drop for MuseSubscription {
    fn drop(&mut self) {
        let Some(host) = self.host.upgrade() else {
            return;
        };
        let mut hub = host.hub.lock().unwrap();
        if let Some(subscribers) = hub.get_mut(&self.session_id) {
            subscribers.retain(|(id, _)| *id != self.subscriber_id);
            if subscribers.is_empty() {
                hub.remove(&self.session_id);
            }
        }
    }
}

fn reader_loop(host: Weak<MuseHost>, mut lines: BufReader<impl std::io::Read>) {
    loop {
        let mut line = String::new();
        // Bound the buffer BEFORE the host fills it: a newline-free stream
        // must not grow `line` past the cap.
        let read = {
            let mut bounded = (&mut lines).take(MAX_LINE_BYTES as u64 + 1);
            bounded.read_line(&mut line)
        };
        match read {
            Ok(0) => break,
            Ok(bytes) if bytes > MAX_LINE_BYTES => {
                log("muse serve emitted an overlong line; treating the host as dead");
                break;
            }
            Ok(_) if line.trim().is_empty() => continue,
            Ok(_) => {}
            Err(_) => break,
        }
        let Ok(frame) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(host) = host.upgrade() else {
            return;
        };
        host.route(&frame);
    }
    if let Some(host) = host.upgrade() {
        host.terminate();
    }
}

fn log(message: &str) {
    eprintln!("waku-muse: {message}");
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};

    /// A `muse` binary that answers the MSP lifecycle and view requests this
    /// client makes, then streams one canned turn per `turn/start`.
    ///
    /// MSP types some params exactly, so requests that drift — a `history`
    /// object, `view/page` with `"cursor": null`, `view/unsubscribe` with no
    /// request id, `session/read` without `excludeItems: false` — are
    /// appended to `<dir>/violations.log`; tests assert it never appears.
    pub(crate) fn fake_muse(directory: &Path) -> PathBuf {
        let binary = directory.join("muse");
        fs::write(
            &binary,
            r#"#!/bin/bash
if [ "$1" = "serve" ]; then
  VIOLATIONS="$(dirname "$0")/violations.log"
  while IFS= read -r line; do
    # Wire-shape assertions before dispatch: these params are typed exactly
    # in the schema, and the driver must send them that way.
    case "$line" in
      *'"view/page"'*'"cursor":null'*)
        echo 'view/page sent cursor:null' >> "$VIOLATIONS" ;;
      *'"history"'*)
        case "$line" in
          *'"history":"'*) ;;
          *) echo 'history preference is not a string' >> "$VIOLATIONS" ;;
        esac ;;
      *'"view/unsubscribe"'*)
        case "$line" in
          *'"id"'*) ;;
          *) echo 'view/unsubscribe sent without a request id' >> "$VIOLATIONS" ;;
        esac ;;
      *'"session/read"'*)
        case "$line" in
          *'"excludeItems":false'*) ;;
          *) echo 'session/read without excludeItems:false' >> "$VIOLATIONS" ;;
        esac ;;
    esac
    case "$line" in
      *'"initialize"'*)
        id=$(echo "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"serverInfo\":{\"name\":\"muse\",\"version\":\"0.0.0-test\"},\"schema\":{\"version\":1,\"fingerprint\":\"sha256:0000000000000000000000000000000000000000000000000000000000000000\"},\"sessionDurability\":\"durable\"}}"
        ;;
      *'"session/start"'*|*'"session/resume"'*|*'"session/read"'*)
        sid=$(echo "$line" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p')
        id=$(echo "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
        history='"mode":"none","items":null,"snapshot":null,"noneReason":"cursorSuffix"'
        case "$line" in
          *'"session/read"'*) history='"mode":"snapshot","items":null,"snapshot":{"state":{}},"noneReason":null' ;;
        esac
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"session\":{\"sessionId\":\"$sid\",\"status\":\"idle\",\"turnCount\":0,\"path\":\"p\",\"providerId\":\"muse\",\"modelId\":\"muse-1\",\"forkedFrom\":null,\"activeTurnId\":null,\"workspaceRoot\":\"/tmp\",\"createdAt\":\"2026-01-01T00:00:00Z\",\"updatedAt\":\"2026-01-01T00:00:00Z\"},\"history\":{$history},\"pendingRequests\":[],\"viewCursor\":\"c9\"}}"
        ;;
      *'"view/page"'*)
        id=$(echo "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"events\":[{\"method\":\"item/completed\",\"params\":{\"sessionId\":\"s1\",\"viewCursor\":\"c1\",\"sourceRange\":{},\"item\":{\"itemId\":\"m1\",\"kind\":\"userMessage\",\"status\":\"completed\",\"text\":\"first prompt\",\"sessionId\":\"s1\",\"turnId\":\"t1\",\"viewCursor\":\"c1\",\"revision\":0,\"recordedAt\":0}}},{\"method\":\"item/completed\",\"params\":{\"sessionId\":\"s1\",\"viewCursor\":\"c2\",\"sourceRange\":{},\"item\":{\"itemId\":\"m2\",\"kind\":\"agentMessage\",\"status\":\"completed\",\"text\":\"first answer\",\"sessionId\":\"s1\",\"turnId\":\"t1\",\"viewCursor\":\"c2\",\"revision\":0,\"recordedAt\":0}}},{\"method\":\"turn/completed\",\"params\":{\"sessionId\":\"s1\",\"turnId\":\"t1\",\"terminal\":\"completed\",\"viewCursor\":\"c3\",\"sourceRange\":{}}}],\"nextCursor\":null}}"
        ;;
      *'"turn/start"'*)
        sid=$(echo "$line" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p')
        id=$(echo "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"commandId\":\"c\",\"status\":\"accepted\",\"disposition\":\"started\",\"startedNewTurn\":true,\"turnId\":\"t1\"}}"
        echo "{\"jsonrpc\":\"2.0\",\"method\":\"turn/started\",\"params\":{\"sessionId\":\"$sid\",\"turnId\":\"t1\",\"viewCursor\":\"c1\"}}"
        echo "{\"jsonrpc\":\"2.0\",\"method\":\"item/started\",\"params\":{\"sessionId\":\"$sid\",\"viewCursor\":\"c2\",\"item\":{\"itemId\":\"m1\",\"kind\":\"agentMessage\",\"status\":\"inProgress\",\"sessionId\":\"$sid\",\"turnId\":\"t1\",\"viewCursor\":\"c2\",\"revision\":0,\"recordedAt\":0}}}"
        echo "{\"jsonrpc\":\"2.0\",\"method\":\"item/delta\",\"params\":{\"sessionId\":\"$sid\",\"itemId\":\"m1\",\"delta\":\"hello \",\"viewCursor\":\"c3\"}}"
        echo "{\"jsonrpc\":\"2.0\",\"method\":\"item/delta\",\"params\":{\"sessionId\":\"$sid\",\"itemId\":\"m1\",\"delta\":\"world\",\"viewCursor\":\"c4\"}}"
        echo "{\"jsonrpc\":\"2.0\",\"method\":\"item/completed\",\"params\":{\"sessionId\":\"$sid\",\"viewCursor\":\"c5\",\"sourceRange\":{},\"item\":{\"itemId\":\"m1\",\"kind\":\"agentMessage\",\"status\":\"completed\",\"text\":\"hello world\",\"sessionId\":\"$sid\",\"turnId\":\"t1\",\"viewCursor\":\"c5\",\"revision\":1,\"recordedAt\":0}}}"
        echo "{\"jsonrpc\":\"2.0\",\"method\":\"session/contextUsage\",\"params\":{\"sessionId\":\"$sid\",\"usedTokens\":42,\"windowTokens\":1000,\"pressure\":\"normal\",\"viewCursor\":\"c6\",\"sourceRange\":{}}}"
        echo "{\"jsonrpc\":\"2.0\",\"method\":\"turn/completed\",\"params\":{\"sessionId\":\"$sid\",\"turnId\":\"t1\",\"terminal\":\"completed\",\"viewCursor\":\"c7\",\"sourceRange\":{}}}"
        ;;
      *)
        id=$(echo "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
        [ -n "$id" ] && echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{}}"
        ;;
    esac
  done
fi
"#,
        )
        .unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        binary
    }
}
