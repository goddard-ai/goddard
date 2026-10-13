//! The IPC server that carries one `RuntimeHost` on its private socket.
//!
//! Four roles share the listener: the daemon's controller connection, the
//! daemon's service endpoint for forwarded agent traffic, narrow local
//! emergency authority, and session-scoped agent ingress. Every request is
//! authorized by what its `Connect` proved, and every mutating call still
//! passes through the host's own fencing — the server adds no authority,
//! only reachability.
//!
//! A pump thread drains provider channels and sweeps ingress deadlines on
//! a fixed interval so truth becomes durable whether or not a controller
//! is attached; waits are long-polls that the pump cycle wakes. Tests can
//! ignore the thread entirely — every request pumps before answering, so
//! `wait_ms: 0` is always deterministic.

use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::contract::*;
use crate::host::RuntimeHost;
use crate::ipc;
use crate::wire::*;

/// What a connection's `Connect` proved.
enum Role {
    /// Full control authority — the product daemon.
    Controller(String),
    /// The daemon's service endpoint for forwarded agent traffic.
    Service(String, u64),
    /// Narrow local authority: status and emergency stop.
    Emergency(String),
    /// Session-scoped agent ingress.
    Agent(SessionGrant),
}

struct Inner {
    host: Mutex<RuntimeHost>,
    /// Wakes long-poll waiters whenever the pump cycle may have changed
    /// something — new events, ingress dispositions, service work.
    changed: Condvar,
    shutdown: AtomicBool,
}

/// Owns the listener and its threads. `shutdown` (or drop) stops them;
/// the host is consumed on bind.
pub struct RuntimeServer {
    inner: Arc<Inner>,
    accept: Option<JoinHandle<()>>,
    emergency: Option<JoinHandle<()>>,
    pump: Option<JoinHandle<()>>,
    root: std::path::PathBuf,
    instance: uuid::Uuid,
}

impl RuntimeServer {
    /// Bind the namespace's private socket, publish `endpoint.json`, and
    /// start serving. The host keeps its OS-held singleton lock for the
    /// server's lifetime.
    pub fn bind(host: RuntimeHost, root: &Path, pump_interval: Duration) -> io::Result<Self> {
        let instance = host.instance();
        let listener = ipc::bind(root)?;
        let emergency_listener = ipc::bind_emergency(root)?;
        ipc::Endpoint::write(root, host.namespace(), host.instance())?;
        let inner = Arc::new(Inner {
            host: Mutex::new(host),
            changed: Condvar::new(),
            shutdown: AtomicBool::new(false),
        });
        let accept = {
            let inner = inner.clone();
            std::thread::Builder::new()
                .name("waku-runtime-accept".into())
                .spawn(move || accept_loop(listener, inner, pump_interval, false))?
        };
        let emergency = {
            let inner = inner.clone();
            std::thread::Builder::new()
                .name("waku-runtime-emergency".into())
                .spawn(move || accept_loop(emergency_listener, inner, pump_interval, true))?
        };
        let pump = {
            let inner = inner.clone();
            std::thread::Builder::new()
                .name("waku-runtime-pump".into())
                .spawn(move || pump_loop(inner, pump_interval))?
        };
        Ok(Self {
            inner,
            accept: Some(accept),
            emergency: Some(emergency),
            pump: Some(pump),
            root: root.into(),
            instance,
        })
    }

    /// Stop accepting and stop the pump. In-flight connection threads
    /// finish their current request and see the flag.
    pub fn is_shutdown(&self) -> bool {
        self.inner.shutdown.load(Ordering::Acquire)
    }

    pub fn shutdown(&mut self) {
        self.inner.shutdown.store(true, Ordering::Release);
        self.inner.changed.notify_all();
        for handle in self
            .accept
            .take()
            .into_iter()
            .chain(self.emergency.take())
            .chain(self.pump.take())
        {
            let _ = handle.join();
        }
        if ipc::Endpoint::load(&self.root).is_ok_and(|endpoint| endpoint.instance == self.instance)
        {
            let _ = std::fs::remove_file(ipc::Endpoint::path(&self.root));
            let _ = std::fs::remove_file(ipc::socket_path(&self.root));
            let _ = std::fs::remove_file(ipc::emergency_socket_path(&self.root));
        }
    }
}

impl Drop for RuntimeServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn pump_loop(inner: Arc<Inner>, interval: Duration) {
    while !inner.shutdown.load(Ordering::Acquire) {
        {
            let mut host = inner.host.lock().unwrap();
            host.pump();
        }
        inner.changed.notify_all();
        std::thread::sleep(interval);
    }
}

fn accept_loop(
    listener: UnixListener,
    inner: Arc<Inner>,
    interval: Duration,
    emergency_only: bool,
) {
    let connections = Arc::new(AtomicUsize::new(0));
    let limit = if emergency_only { 2 } else { 8 };
    let _ = listener.set_nonblocking(true);
    loop {
        if inner.shutdown.load(Ordering::Acquire) {
            return;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                // Same-user peer verification is the private channel's
                // first gate — before any credential is even read.
                // On BSD the accepted socket inherits the listener's
                // non-blocking flag; connections must block (bounded by
                // the read timeout) or frame reads desync.
                let _ = stream.set_nonblocking(false);
                if !ipc::same_user(&stream).unwrap_or(false) {
                    continue;
                }
                if connections.load(Ordering::Acquire) >= limit {
                    continue;
                }
                connections.fetch_add(1, Ordering::AcqRel);
                let count = connections.clone();
                let inner = inner.clone();
                let spawned = std::thread::Builder::new()
                    .name("waku-runtime-conn".into())
                    .spawn(move || {
                        struct Slot(Arc<AtomicUsize>);
                        impl Drop for Slot {
                            fn drop(&mut self) {
                                self.0.fetch_sub(1, Ordering::AcqRel);
                            }
                        }
                        let _slot = Slot(count);
                        handle_conn(stream, inner, emergency_only);
                    });
                if spawned.is_err() {
                    connections.fetch_sub(1, Ordering::AcqRel);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(interval),
            Err(_) => return,
        }
    }
}

fn send(stream: &mut UnixStream, response: &WireResponse) -> io::Result<()> {
    ipc::send(stream, response)
}

fn error(error: HostError) -> WireResponse {
    WireResponse::Error { error }
}

fn handle_conn(mut stream: UnixStream, inner: Arc<Inner>, emergency_only: bool) {
    // A bounded read timeout lets connection threads notice shutdown; a
    // timeout is a retry, never a disconnect.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let connect: Connect = match ipc::recv(&mut stream) {
        Ok(Some(c)) => c,
        _ => return,
    };
    if emergency_only && !matches!(&connect.auth, ConnectAuth::Emergency { .. }) {
        let _ = send(&mut stream, &error(HostError::Unauthorized));
        return;
    }
    let role = {
        let mut host = inner.host.lock().unwrap();
        if connect.namespace != host.namespace() {
            let _ = send(&mut stream, &error(HostError::Unauthorized));
            return;
        }
        match &connect.auth {
            ConnectAuth::Controller { credential, client } => {
                match host.hello(credential, client) {
                    Ok(hello) => {
                        if send(
                            &mut stream,
                            &WireResponse::Hello {
                                hello: Box::new(hello),
                            },
                        )
                        .is_err()
                        {
                            return;
                        }
                        Role::Controller(credential.clone())
                    }
                    Err(e) => {
                        let _ = send(&mut stream, &error(e));
                        return;
                    }
                }
            }
            ConnectAuth::Service {
                credential,
                epoch,
                client,
            } => {
                // The service binds the epoch it answers for; a stale
                // epoch never starts serving and every call is fenced
                // again per request.
                let result = host
                    .hello(credential, client)
                    .ok()
                    .filter(|_| *epoch == host.epoch());
                match result {
                    Some(hello) => {
                        if send(
                            &mut stream,
                            &WireResponse::Hello {
                                hello: Box::new(hello),
                            },
                        )
                        .is_err()
                        {
                            return;
                        }
                        Role::Service(credential.clone(), *epoch)
                    }
                    None => {
                        let _ = send(&mut stream, &error(HostError::Unauthorized));
                        return;
                    }
                }
            }
            ConnectAuth::Emergency { credential } => match host.status(credential) {
                Ok(status) => {
                    if send(
                        &mut stream,
                        &WireResponse::Status {
                            status: Box::new(status),
                        },
                    )
                    .is_err()
                    {
                        return;
                    }
                    Role::Emergency(credential.clone())
                }
                Err(e) => {
                    let _ = send(&mut stream, &error(e));
                    return;
                }
            },
            ConnectAuth::Agent { grant } => match host.check_grant(grant) {
                Ok(_) => {
                    if send(&mut stream, &WireResponse::Ok).is_err() {
                        return;
                    }
                    Role::Agent(grant.clone())
                }
                Err(e) => {
                    let _ = send(&mut stream, &error(e));
                    return;
                }
            },
        }
    };
    loop {
        if inner.shutdown.load(Ordering::Acquire) {
            return;
        }
        let request: WireRequest = match ipc::recv(&mut stream) {
            Ok(Some(r)) => r,
            Ok(None) => return,
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(_) => {
                eprintln!("waku-runtime: dropping malformed or closed connection");
                return;
            }
        };
        let response = respond(&inner, &role, request);
        let drained = matches!(
            response,
            WireResponse::Drain {
                report: DrainReport::Drained { .. }
            }
        );
        let delivered = send(&mut stream, &response);
        if drained {
            inner.shutdown.store(true, Ordering::Release);
            inner.changed.notify_all();
        }
        if delivered.is_err() {
            return;
        }
    }
}

fn respond(inner: &Arc<Inner>, role: &Role, request: WireRequest) -> WireResponse {
    macro_rules! controller {
        () => {
            match role {
                Role::Controller(credential) => credential,
                _ => return error(HostError::Unauthorized),
            }
        };
    }
    macro_rules! service {
        ($epoch:expr) => {
            match role {
                Role::Service(credential, bound_epoch) => {
                    let current = inner.host.lock().unwrap().epoch();
                    if current != *bound_epoch || $epoch != *bound_epoch {
                        return error(HostError::StaleEpoch {
                            current,
                            presented: *bound_epoch,
                        });
                    }
                    credential
                }
                _ => return error(HostError::Unauthorized),
            }
        };
    }
    match request {
        WireRequest::Hello { client } => {
            let credential = controller!();
            let mut host = inner.host.lock().unwrap();
            host.hello(credential, &client)
                .map(|h| WireResponse::Hello { hello: Box::new(h) })
                .unwrap_or_else(error)
        }
        WireRequest::AcquireControl { client } => {
            let credential = controller!();
            let mut host = inner.host.lock().unwrap();
            host.acquire_control(credential, &client)
                .map(|grant| WireResponse::Control { grant })
                .unwrap_or_else(error)
        }
        WireRequest::Reconcile { epoch } => {
            let credential = controller!();
            let mut host = inner.host.lock().unwrap();
            host.reconcile(credential, epoch)
                .map(|i| WireResponse::Inventory {
                    inventory: Box::new(i),
                })
                .unwrap_or_else(error)
        }
        WireRequest::Execute { envelope } => {
            let credential = controller!();
            let mut host = inner.host.lock().unwrap();
            host.execute(credential, *envelope)
                .map(|receipt| WireResponse::Receipt { receipt })
                .unwrap_or_else(error)
        }
        WireRequest::Command { command_id } => {
            let credential = controller!();
            let host = inner.host.lock().unwrap();
            WireResponse::Command {
                receipt: host.command(credential, command_id),
            }
        }
        WireRequest::Events {
            epoch,
            cursor,
            wait_ms,
        } => {
            let credential = controller!();
            let deadline = Instant::now() + Duration::from_millis(wait_ms.min(30_000));
            loop {
                let mut host = inner.host.lock().unwrap();
                match host.events(credential, epoch, cursor) {
                    Ok(mut page) => {
                        if !page.events.is_empty() || wait_ms == 0 || Instant::now() >= deadline {
                            // Keep the original high-water mark. The omitted suffix
                            // remains journaled and is fetched after the last applied
                            // sequence; pagination never acknowledges unseen events.
                            let mut bytes = 8192usize;
                            let mut count = 0;
                            for event in &page.events {
                                let size = match serde_json::to_vec(event) {
                                    Ok(value) => value.len(),
                                    Err(_) => {
                                        return error(HostError::Malformed("event encoding".into()));
                                    }
                                };
                                if bytes.saturating_add(size + 1) > ipc::MAX_FRAME_BYTES {
                                    break;
                                }
                                bytes += size + 1;
                                count += 1;
                            }
                            if count == 0 && !page.events.is_empty() {
                                return error(HostError::Capacity(
                                    "event exceeds IPC frame budget".into(),
                                ));
                            }
                            page.events.truncate(count);
                            return WireResponse::Page {
                                page: Box::new(page),
                            };
                        }
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        let (guard, _) = inner.changed.wait_timeout(host, remaining).unwrap();
                        drop(guard);
                    }
                    Err(e) => return error(e),
                }
            }
        }
        WireRequest::Acknowledge { epoch, cursor } => {
            let credential = controller!();
            let mut host = inner.host.lock().unwrap();
            host.acknowledge(credential, epoch, cursor)
                .map(|()| WireResponse::Ok)
                .unwrap_or_else(error)
        }
        WireRequest::Status => {
            let credential = match role {
                Role::Controller(c) | Role::Service(c, _) | Role::Emergency(c) => c,
                Role::Agent(_) => return error(HostError::Unauthorized),
            };
            let mut host = inner.host.lock().unwrap();
            host.status(credential)
                .map(|s| WireResponse::Status {
                    status: Box::new(s),
                })
                .unwrap_or_else(error)
        }
        WireRequest::Drain { epoch, policy } => {
            let credential = controller!();
            let mut host = inner.host.lock().unwrap();
            host.drain(credential, epoch, policy)
                .map(|report| WireResponse::Drain { report })
                .unwrap_or_else(error)
        }
        WireRequest::EmergencyStop { session_id } => {
            let credential = match role {
                Role::Emergency(c) => c,
                _ => return error(HostError::Unauthorized),
            };
            let mut host = inner.host.lock().unwrap();
            host.emergency_stop(credential, session_id)
                .map(|status| WireResponse::SessionStatus { status })
                .unwrap_or_else(error)
        }
        WireRequest::IssueGrant {
            epoch,
            session,
            ttl_ms,
        } => {
            let credential = controller!();
            let mut host = inner.host.lock().unwrap();
            host.issue_grant(credential, epoch, session, ttl_ms)
                .map(|g| WireResponse::Grant { grant: Box::new(g) })
                .unwrap_or_else(error)
        }
        WireRequest::RegisterClaim { epoch, claim } => {
            let credential = controller!();
            let mut host = inner.host.lock().unwrap();
            host.register_claim(credential, epoch, claim)
                .map(|()| WireResponse::Ok)
                .unwrap_or_else(error)
        }
        WireRequest::ReleaseClaim { epoch, claim_id } => {
            let credential = controller!();
            let mut host = inner.host.lock().unwrap();
            host.release_claim(credential, epoch, claim_id)
                .map(|()| WireResponse::Ok)
                .unwrap_or_else(error)
        }
        WireRequest::ServiceDrain { epoch, wait_ms } => {
            let credential = service!(epoch);
            let deadline = Instant::now() + Duration::from_millis(wait_ms.min(30_000));
            loop {
                let mut host = inner.host.lock().unwrap();
                match host.ingress_drain(credential, epoch) {
                    Ok(requests) => {
                        if !requests.is_empty() || wait_ms == 0 || Instant::now() >= deadline {
                            return WireResponse::Forwarded { requests };
                        }
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        let (guard, _) = inner.changed.wait_timeout(host, remaining).unwrap();
                        drop(guard);
                    }
                    Err(e) => return error(e),
                }
            }
        }
        WireRequest::ServiceResult {
            epoch,
            request_id,
            outcome,
        } => {
            let credential = service!(epoch);
            let mut host = inner.host.lock().unwrap();
            host.ingress_resolve(credential, epoch, request_id, outcome)
                .map(|r| WireResponse::Ingress {
                    record: Box::new(r),
                })
                .unwrap_or_else(error)
        }
        WireRequest::Ingress { request } => {
            let grant = match role {
                Role::Agent(grant) => grant,
                _ => return error(HostError::Unauthorized),
            };
            let mut host = inner.host.lock().unwrap();
            host.ingress_submit(grant, request)
                .map(|r| WireResponse::Ingress {
                    record: Box::new(r),
                })
                .unwrap_or_else(error)
        }
        WireRequest::IngressQuery { request_id } => {
            let grant = match role {
                Role::Agent(grant) => grant,
                _ => return error(HostError::Unauthorized),
            };
            let mut host = inner.host.lock().unwrap();
            host.ingress_query(grant, request_id)
                .map(|r| WireResponse::Ingress {
                    record: Box::new(r),
                })
                .unwrap_or_else(error)
        }
        WireRequest::IngressWait {
            request_id,
            wait_ms,
        } => {
            let grant = match role {
                Role::Agent(grant) => grant,
                _ => return error(HostError::Unauthorized),
            };
            let deadline = Instant::now() + Duration::from_millis(wait_ms.min(30_000));
            loop {
                let mut host = inner.host.lock().unwrap();
                match host.ingress_query(grant, request_id) {
                    Ok(record) => {
                        let live = matches!(
                            record.disposition,
                            IngressDisposition::Pending | IngressDisposition::Forwarded
                        );
                        if !live || wait_ms == 0 || Instant::now() >= deadline {
                            return WireResponse::Ingress {
                                record: Box::new(record),
                            };
                        }
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        let (guard, _) = inner.changed.wait_timeout(host, remaining).unwrap();
                        drop(guard);
                    }
                    Err(e) => return error(e),
                }
            }
        }
    }
}
