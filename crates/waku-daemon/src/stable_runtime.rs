//! The daemon's opt-in link to a surviving provider-runtime host.
//!
//! One `StableLink` is the daemon-side controller for one host session: it
//! implements [`driver::DriverControl`] so the rest of the daemon — forwarder,
//! projections, transcript recording — sees the same driver surface a local
//! provider process presents, while the provider itself lives under the
//! separately surviving `waku-runtime-host` process.
//!
//! Semantics the link owes the rest of the daemon:
//!
//! - Attach, don't resume: a task whose durable link row still names a live
//!   host session reconnects to that session's existing work instead of
//!   admitting a new provider turn.
//! - Dedup before effects: a command's recorded truth (`client.command`) is
//!   consulted after transport loss rather than resending under a new
//!   identity, and forwarded agent requests answer from durable receipts.
//! - Durable before ack: the daemon only acknowledges a host event sequence
//!   after the forwarder has applied it — the marker the pump interleaves is
//!   the ack boundary, and `stable_runtime_links.applied_seq` is the
//!   crash-resume truth.
//! - Uncertain, never resent: effects the daemon cannot confirm stay
//!   `Uncertain` — no automatic resend on either side.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use serde_json::{Value, json};
use uuid::Uuid;
use waku_protocol::model::RuntimeEventCursor;
use crate::{Command, Request};
use waku_runtime::client::{ClientError, RuntimeClient};
use waku_runtime::contract::{
    ActionEnvelope, CanonicalAction, ClientHello, ControlGrant, EventCursor, EventEnvelope, ForwardedOutcome,
    ForwardedRequest, Inventory, ProviderEvent, RequestClass, SessionRef, SessionStatus,
};
use waku_runtime::host::load_credentials;

use crate::daemon::WakuBackend;
use crate::driver::{DriverControl, DriverEventSender, DriverHandle, DriverStartOptions};
use crate::model::{DriverEvent, PermissionOption, ProviderKind, UserInputQuestion};
use crate::persistence::{StableLinkRow, StableReceipt, StateStore};
use crate::Backend;

/// The namespace the Goddard daemon's controller owns on the host.
pub(crate) const STABLE_NAMESPACE: &str = "goddard";

/// Env override naming the namespace root — the explicit Stage-2C opt-in.
const ROOT_ENV: &str = "WAKU_STABLE_RUNTIME_ROOT";
/// Env override listing providers routed through the host — `all` or a
/// comma-separated provider id list. Defaults to every provider when the
/// root opt-in is set.
const PROVIDERS_ENV: &str = "WAKU_STABLE_RUNTIME_PROVIDERS";
/// Env override locating the `waku-runtime-host` executable to spawn when
/// no live endpoint answers. Absent it, the sibling of the daemon binary is
/// tried; a connect that still fails reports the truth.
const HOST_BIN_ENV: &str = "WAKU_RUNTIME_HOST_BIN";
/// How long an events/service long-poll blocks — bounded so a stopping link
/// observes its flag between polls.
const POLL_WAIT_MS: u64 = 200;
/// How long to wait for a freshly spawned host to publish its endpoint.
const HOST_READY_WAIT: Duration = Duration::from_secs(15);

/// The default runtime namespace root under the daemon's data directory.
pub(crate) fn default_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join(STABLE_NAMESPACE)
}

/// Whether this launch should ride the stable runtime. The env root opts in
/// explicitly; a provider list without a root uses the default namespace
/// under the daemon's data dir. Provider gating defaults to every provider
/// the host has an adapter for — an unregistered adapter fails the
/// admission loudly rather than silently falling back to a legacy driver.
pub(crate) fn enabled(
    provider: ProviderKind,
    data_dir: &Path,
    test_root: Option<&PathBuf>,
) -> Option<PathBuf> {
    let root = test_root
        .cloned()
        .or_else(|| std::env::var(ROOT_ENV).ok().filter(|v| !v.is_empty()).map(PathBuf::from));
    let list = std::env::var(PROVIDERS_ENV).ok();
    let allowed = match list.as_deref() {
        // A root alone opts in for every provider the host can serve.
        None => root.is_some(),
        Some(list) => list.trim() == "all" || list.split(',').any(|p| p.trim() == provider.id()),
    };
    if !allowed {
        return None;
    }
    root.or_else(|| list.is_some().then(|| default_root(data_dir)))
}

/// The daemon's executor for forwarded agent requests — the product-policy
/// half of the ingress bridge. The runtime knows request shape and scope;
/// the daemon alone decides what a request does.
pub(crate) trait IngressService: Send + Sync {
    /// Execute the request and produce the outcome the runtime records. An
    /// executor that cannot confirm completion must return `Uncertain`,
    /// never a lie.
    fn execute(&self, request: &ForwardedRequest) -> ForwardedOutcome;
}

/// Default executor: `subject` carries `{"command": <Command>}` dispatched
/// through the same scoped-agent path a provider-side `goddard-agent` uses —
/// one authorization surface, no parallel semantics.
struct BackendIngress {
    backend: Weak<WakuBackend>,
    task_id: Uuid,
    runtime_id: Uuid,
}

impl IngressService for BackendIngress {
    fn execute(&self, request: &ForwardedRequest) -> ForwardedOutcome {
        let Some(backend) = self.backend.upgrade() else {
            return ForwardedOutcome::Failed {
                detail: "the daemon's ingress service is gone".into(),
            };
        };
        let command: Command = match serde_json::from_value(request.subject.clone()) {
            Ok(command) => command,
            Err(error) => {
                return ForwardedOutcome::Failed {
                    detail: format!("ingress subject is not a scoped command: {error}"),
                };
            }
        };
        let events = backend.event_source();
        match backend.handle(
            Request {
                request_id: request.request_id,
                session_id: self.task_id,
                runtime_id: self.runtime_id,
                command,
            },
            events,
            Some(self.task_id),
        ) {
            Ok(payload) => ForwardedOutcome::Completed {
                result: serde_json::to_value(&payload).unwrap_or(Value::Null),
            },
            Err(error) => ForwardedOutcome::Failed {
                detail: format!("{error:#}"),
            },
        }
    }
}

/// A shared handle to one host attachment. `Clone` so the driver control and
/// the forwarder's marker routing reach the same state.
#[derive(Clone)]
pub(crate) struct StableLink {
    shared: Arc<LinkShared>,
    // Workers retain LinkShared but never this lifetime token. A temporary
    // forwarder clone must not detach the owning driver and registry entry.
    _lifetime: Arc<LinkLifetime>,
}

struct LinkLifetime(Arc<AtomicBool>);
impl Drop for LinkLifetime {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(crate) struct ControllerLease {
    instance: Uuid,
    grant: ControlGrant,
    routes: Mutex<HashMap<Uuid, Weak<LinkShared>>>,
    /// All service connections share dispatch serialization.
    service_gate: Mutex<()>,
}

struct LinkShared {
    controller: Arc<ControllerLease>,
    provider: ProviderKind,
    mode: crate::model::RuntimeMode,
    /// The daemon task this session serves.
    task_id: Uuid,
    /// The daemon's runtime-attempt id — also the forwarder's owner tag.
    runtime_id: Uuid,
    /// The host session and generation this link is attached to.
    sref: SessionRef,
    namespace: String,
    /// The host instance that admitted the session.
    instance: Uuid,
    /// The controller epoch this link holds — stale the moment a newer
    /// daemon attaches, and every mutation under it is fenced.
    epoch: u64,
    retry_window_id: Uuid,
    /// Correlates this link's in-band applied markers: another attempt's
    /// `RuntimeEventCursorAdvanced` cannot move this link's cursor.
    marker_epoch: Uuid,
    /// The controller credential — pump and service reconnect as needed.
    credential: String,
    /// The namespace root — needed to reconnect.
    root: PathBuf,
    /// Command-path connection. `Mutex` — `RuntimeClient` is `&mut`-driven.
    client: Mutex<RuntimeClient>,
    store: Arc<StateStore>,
    /// Provider events flow into the daemon's ordinary forwarder channel.
    sender: DriverEventSender,
    service: Arc<dyn IngressService>,
    /// The host turn the last `Submit` opened — `cancel` cancels it.
    active_turn: Mutex<Option<Uuid>>,
    /// The last configuration revision the link requested — reconfigure is
    /// monotone, never rolled back by a stale caller.
    config_revision: Mutex<u64>,
    /// Set on drop — pump and service threads retire between polls.
    stop: Arc<AtomicBool>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

/// Attach a daemon runtime attempt to the host and return its driver handle.
/// The whole reconcile-then-attach sequence lives here so `start_runtime_driver`
/// stays a provider-agnostic dispatch.
#[allow(clippy::too_many_arguments)]
pub(super) fn start_link(
    backend: &WakuBackend,
    task_store: &Arc<StateStore>,
    session_id: Uuid,
    runtime_id: Uuid,
    provider: ProviderKind,
    options: &DriverStartOptions,
    sender: DriverEventSender,
    root: &Path,
    service_override: Option<Arc<dyn IngressService>>,
) -> anyhow::Result<(DriverHandle, StableLink)> {
    waku_runtime::ipc::private_root(root).context("could not verify the private runtime namespace root")?;
    let verified_root = root.canonicalize().context("could not resolve the runtime namespace")?;
    let root = verified_root.as_path();
    ensure_host(root)?;
    let creds = load_credentials(root).context("the runtime host's identity is unreadable")?;
    let (mut client, hello) = RuntimeClient::controller(root, &creds.controller, &ClientHello::current())
        .map_err(link_error)
        .context("could not attach to the runtime host")?;
    let instance = hello.runtime_instance;
    let controller = {
        let mut leases = backend.stable_controllers.lock().unwrap();
        if let Some(lease) = leases.get(root) {
            anyhow::ensure!(lease.instance == instance, "the runtime instance changed; reconciliation is required before admission");
            lease.clone()
        } else {
            let grant = client.acquire_control(&ClientHello::current())
                .map_err(link_error).context("the runtime host refused control")?;
            let lease = Arc::new(ControllerLease {
                instance,
                grant,
                routes: Mutex::new(HashMap::new()),
                service_gate: Mutex::new(()),
            });
            leases.insert(root.to_path_buf(), lease.clone());
            lease
        }
    };
    let grant = &controller.grant;
    // Reconciliation precedes admission: the inventory names live sessions,
    // open commands, pending requests, surviving claims — a replacement
    // daemon acts on recorded truth, never on what it hoped was running.
    let inventory = client
        .reconcile(grant.epoch)
        .map_err(link_error)
        .context("the runtime host refused reconciliation")?;
    for inv in &inventory.sessions {
        // Sessions the daemon doesn't own stay live — Stage 2C preserves
        // every survivor rather than guessing at teardown authority.
        if inv.session.session_id != runtime_id {
            eprintln!(
                "goddard-daemon: runtime session {} ({:?}) is preserved from a prior attachment",
                inv.session.session_id, inv.status
            );
        }
    }

    let prior = task_store
        .stable_link(session_id)
        .context("could not read the task's runtime link state")?;
    let attach = resolve_attachment(prior.as_ref(), &inventory, provider)?;
    let (sref, mut row) = match attach {
        Attachment::Reattach { sref, row } => {
            // The host's session is still live under the same identity —
            // attach to its existing work; nothing is resumed or replayed
            // as a new turn.
            (sref, row)
        }
        Attachment::Admit { mut row } => {
            let adapter = provider.id().to_string();
            // The runtime session id is the daemon's attempt id and the
            // admit's command id is the same value: a transport-lost admit
            // answers from its recorded receipt, never double-spawns.
            let action = CanonicalAction::AdmitSession {
                session_id: runtime_id,
                adapter,
                launch: launch_payload(provider, options)?,
                requested: requested_config(provider, options),
            };
            let receipt = execute_safe(
                &mut client,
                &ActionEnvelope {
                    namespace: STABLE_NAMESPACE.into(),
                    runtime_instance: instance,
                    epoch: grant.epoch,
                    retry_window_id: grant.retry_window_id,
                    // The attempt id doubles as the command's durable identity.
                    command_id: runtime_id,
                    session: None,
                    action,
                },
            )
            .map_err(|e| anyhow!("runtime admission could not be confirmed: {e}"))?;
            match &receipt.disposition {
                waku_runtime::contract::CommandDisposition::Terminal { .. } => {}
                other => {
                    bail!("the runtime host refused the session admission: {other:?}")
                }
            }
            row.runtime_session_id = runtime_id;
            row.generation = 1;
            row.applied_seq = 0;
            row.status = "live".into();
            (SessionRef { session_id: runtime_id, generation: 1 }, row)
        }
    };
    row.session_id = session_id;
    row.updated_at = crate::model::unix_time();
    task_store
        .stable_link_save(&row)
        .context("could not record the runtime link")?;

    let service: Arc<dyn IngressService> = service_override.unwrap_or_else(|| {
        Arc::new(BackendIngress {
            backend: backend.weak_self(),
            task_id: session_id,
            runtime_id,
        })
    });
    let shared = Arc::new(LinkShared {
        controller: controller.clone(),
        provider,
        mode: options.mode,
        task_id: session_id,
        runtime_id,
        sref,
        namespace: STABLE_NAMESPACE.into(),
        instance,
        epoch: grant.epoch,
        retry_window_id: grant.retry_window_id,
        marker_epoch: Uuid::new_v4(),
        credential: creds.controller.clone(),
        root: root.to_path_buf(),
        client: Mutex::new(client),
        store: task_store.clone(),
        sender,
        service,
        active_turn: Mutex::new(inventory.sessions.iter().find(|inv| inv.session == sref).and_then(|inv| inv.open_turns.first().copied())),
        config_revision: Mutex::new(inventory_config_revision(&inventory, &sref)),
        stop: Arc::new(AtomicBool::new(false)),
        threads: Mutex::new(Vec::new()),
    });
    // Re-acknowledge the durable cursor up front: a host that kept events
    // past a lost ack replays them; replayed events arrive under the same
    // sequence and dedup at the forwarder.
    {
        let mut client = shared.client.lock().unwrap();
        let _ = client.acknowledge(shared.epoch, shared.cursor());
    }
    let link = StableLink {
        shared: shared.clone(),
        _lifetime: Arc::new(LinkLifetime(shared.stop.clone())),
    };
    controller.routes.lock().unwrap().insert(sref.session_id, Arc::downgrade(&shared));
    spawn_pump(&shared);
    spawn_service(&shared);
    Ok((DriverHandle::from_control(Arc::new(link.clone())), link))
}

/// Whether the task reattaches to a surviving host session or admits a new
/// one. A recorded row that still names a live host session under the same
/// instance is an attach. Interrupted/replaced owners require explicit
/// recovery; only an unbound task can make a fresh admission.
enum Attachment {
    Reattach { sref: SessionRef, row: StableLinkRow },
    Admit { row: StableLinkRow },
}

fn resolve_attachment(
    prior: Option<&StableLinkRow>,
    inventory: &Inventory,
    provider: ProviderKind,
) -> anyhow::Result<Attachment> {
    if let Some(row) = prior {
        if row.runtime_instance == inventory.runtime_instance
            && let Some(inv) = inventory
                .sessions
                .iter()
                .find(|inv| inv.session.session_id == row.runtime_session_id)
            && matches!(inv.status, SessionStatus::Live | SessionStatus::Stopping)
        {
            anyhow::ensure!(
                inv.adapter == provider.id(),
                "the live runtime belongs to another provider; stop confirmation is required before replacement"
            );
            let mut row = row.clone();
            row.generation = inv.session.generation;
            return Ok(Attachment::Reattach {
                sref: inv.session,
                row,
            });
        }
        bail!(
            "the recorded runtime generation is interrupted or unavailable; explicit recovery is required"
        );
    }
    Ok(Attachment::Admit {
        row: StableLinkRow::admit(
            STABLE_NAMESPACE,
            Uuid::nil(),
            Uuid::nil(),
            inventory.runtime_instance,
        ),
    })
}

/// Match the strict, independently versioned adapter schemas. Unsupported
/// product options fail before admission instead of silently losing policy.
fn launch_payload(provider: ProviderKind, options: &DriverStartOptions) -> anyhow::Result<Value> {
    anyhow::ensure!(
        options.provider_cursor.is_none(),
        "the stable runtime cannot resume a legacy provider session"
    );
    anyhow::ensure!(
        options.agent.is_none(),
        "stable runtime product CLI credentials are not qualified"
    );
    anyhow::ensure!(
        options.sandbox.is_none(),
        "stable runtime sandbox ownership is not qualified"
    );
    anyhow::ensure!(
        !options.computer_use_enabled && options.computer_use_runtime.is_none(),
        "stable runtime Computer Use is not qualified"
    );
    anyhow::ensure!(
        options.mcp_servers.is_empty(),
        "stable runtime MCP routing is not qualified"
    );
    anyhow::ensure!(
        options
            .subagents
            .as_ref()
            .is_none_or(|spec| spec.agents.is_empty())
            && options.agent_preset.is_none(),
        "stable runtime provider presets are not supported"
    );
    anyhow::ensure!(
        options.service_tier.is_none() && options.context_window.is_none(),
        "stable runtime service tier/context window options are not supported"
    );
    match provider {
        ProviderKind::Codex => {
            let (approval, sandbox) = match options.mode {
                crate::model::RuntimeMode::Ask => ("untrusted", "read-only"),
                crate::model::RuntimeMode::AutoAcceptEdits => ("on-request", "workspace-write"),
                crate::model::RuntimeMode::FullAccess => ("never", "danger-full-access"),
                crate::model::RuntimeMode::Auto => {
                    bail!("stable Codex automated approval review is not qualified")
                }
            };
            Ok(json!({"binary": options.binary, "cwd": options.cwd,
                "approvalPolicy": approval, "sandbox": sandbox}))
        }
        ProviderKind::Devin => {
            anyhow::ensure!(
                options.mode == crate::model::RuntimeMode::Ask,
                "stable Devin automatic permissions are not qualified; use supervised mode"
            );
            Ok(json!({"cwd": options.cwd}))
        }
        _ => bail!("no qualified stable launch schema for {}", provider.id()),
    }
}

fn requested_config(
    provider: ProviderKind,
    options: &DriverStartOptions,
) -> waku_runtime::contract::RequestedConfig {
    waku_runtime::contract::RequestedConfig {
        provider: Some(provider.id().into()),
        model: options.model.clone(),
        effort: options.reasoning_effort.clone(),
        options: Value::Null,
    }
}

fn inventory_config_revision(inventory: &Inventory, sref: &SessionRef) -> u64 {
    inventory
        .sessions
        .iter()
        .find(|inv| inv.session == *sref)
        .map(|inv| inv.requested_revision.max(inv.effective.as_ref().map_or(0, |config| config.revision)))
        .unwrap_or(0)
}

/// Spawn or verify the host process for `root`. A live endpoint answers
/// immediately; a stale or absent endpoint gets one spawn attempt with a
/// bounded wait, then the truth.
fn ensure_host(root: &Path) -> anyhow::Result<()> {
    if host_answers(root) {
        return Ok(());
    }
    let binary = std::env::var(HOST_BIN_ENV)
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join("waku-runtime-host")))
        });
    let Some(binary) = binary.filter(|path| path.is_file()) else {
        bail!("no live runtime host answers at {} and no waku-runtime-host binary is available", root.display());
    };
    std::process::Command::new(binary)
        .arg("--root")
        .arg(root)
        .arg("--namespace")
        .arg(STABLE_NAMESPACE)
        .spawn()
        .context("could not spawn the runtime host")?;
    let deadline = std::time::Instant::now() + HOST_READY_WAIT;
    while std::time::Instant::now() < deadline {
        if host_answers(root) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    bail!("the runtime host never published its endpoint at {}", root.display())
}

fn host_answers(root: &Path) -> bool {
    let Ok(creds) = load_credentials(root) else {
        return false;
    };
    RuntimeClient::emergency(root, &creds.emergency).is_ok()
}

/// Translate one host event into the driver event the daemon's forwarder
/// applies. Events with no client-visible truth return `None` — the link
/// still counts them toward the applied sequence.
fn translate(env: &EventEnvelope) -> Option<DriverEvent> {
    Some(match &env.event {
        ProviderEvent::SessionLive => DriverEvent::Connected {
            provider_cursor: None,
        },
        ProviderEvent::TurnStarted { turn_id, .. } => {
            let _ = turn_id;
            DriverEvent::TurnStarted
        }
        ProviderEvent::TurnDelta { delta, .. } => DriverEvent::TextDelta(delta.as_str()?.to_owned()),
        ProviderEvent::TurnCompleted { result, .. } => DriverEvent::TurnFinished {
            success: result.as_str().is_some()
                || result.pointer("/turn/status").and_then(Value::as_str) == Some("completed")
                || result.get("stopReason").and_then(Value::as_str) == Some("end_turn")
                || result.get("status").and_then(Value::as_str) == Some("done"),
            summary: result.pointer("/turn/error/message").and_then(Value::as_str)
                .or_else(|| result.as_str()).map(str::to_owned),
            summary_i18n: None,
        },
        ProviderEvent::TurnCancelled { .. } => DriverEvent::TurnFinished {
            success: false,
            summary: None,
            summary_i18n: None,
        },
        ProviderEvent::RequestOpened {
            request_id,
            class,
            subject,
            ..
        } => match class {
            RequestClass::Input => DriverEvent::UserInputRequested {
                request_id: request_id.clone(),
                questions: serde_json::from_value::<Vec<UserInputQuestion>>(
                    subject.get("questions").cloned().unwrap_or(Value::Array(vec![])),
                )
                .unwrap_or_default(),
            },
            RequestClass::Tool | RequestClass::Approval => DriverEvent::Permission {
                request_id: request_id.clone(),
                title: subject
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("the provider is asking for approval")
                    .to_string(),
                title_i18n: None,
                detail: subject
                    .get("detail")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                detail_i18n: None,
                options: subject
                    .get("options")
                    .and_then(Value::as_array)
                    .map(|options| {
                        options
                            .iter()
                            .filter_map(|option| {
                                Some(PermissionOption {
                                    id: option.get("id")?.as_str()?.to_string(),
                                    label: option
                                        .get("label")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string(),
                                    label_i18n: None,
                                    allow: true,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            },
        },
        ProviderEvent::RequestAnswered { request_id } => DriverEvent::RequestSettled {
            request_id: request_id.clone(),
        },
        // Configuration is projected separately from transcript content.
        ProviderEvent::ConfigActivated { .. } => return None,
        ProviderEvent::ProviderFailed { detail } => DriverEvent::Error(detail.clone()),
        ProviderEvent::Terminated { .. } => DriverEvent::ProcessExited,
        ProviderEvent::Dropped { reason, bytes } => DriverEvent::Error(format!(
            "the runtime dropped an oversized provider message ({reason:?}, {bytes} bytes)"
        )),
    })
}

/// One request/response against the host with transport-loss truth: a lost
/// answer is recovered from the command's recorded receipt, and a command
/// that never landed retries once under the same identity.
fn execute_safe(
    client: &mut RuntimeClient,
    envelope: &ActionEnvelope,
) -> Result<waku_runtime::contract::CommandReceipt, ClientError> {
    match client.execute(envelope.clone()) {
        Err(ClientError::Io(_)) => match client.command(envelope.command_id) {
            Ok(Some(receipt)) => Ok(receipt),
            Ok(None) => client.execute(envelope.clone()),
            Err(e) => Err(e),
        },
        result => result,
    }
}

impl LinkShared {
    fn cursor(&self) -> EventCursor {
        let applied = self
            .store
            .stable_link(self.task_id)
            .ok()
            .flatten()
            .map(|row| row.applied_seq)
            .unwrap_or(0);
        EventCursor {
            session_id: self.sref.session_id,
            generation: self.sref.generation,
            applied,
        }
    }

    fn envelope(&self, command_id: Uuid, action: CanonicalAction) -> ActionEnvelope {
        ActionEnvelope {
            namespace: self.namespace.clone(),
            runtime_instance: self.instance,
            epoch: self.epoch,
            retry_window_id: self.retry_window_id,
            command_id,
            session: Some(self.sref),
            action,
        }
    }

    /// One command against the host. `Command::Prompt`-style identities are
    /// chosen by the caller and stay stable across transport loss; failures
    /// surface as error events, never silent drops.
    fn command(&self, command_id: Uuid, action: CanonicalAction) {
        let mut client = self.client.lock().unwrap();
        match execute_safe(&mut client, &self.envelope(command_id, action)) {
            Ok(receipt) => {
                match receipt.disposition {
                    waku_runtime::contract::CommandDisposition::Rejected { code, detail } => {
                        let _ = self.sender.send(DriverEvent::Error(format!(
                            "runtime command rejected ({code:?}): {detail}"
                        )));
                    }
                    waku_runtime::contract::CommandDisposition::Uncertain { stage, detail } => {
                        let _ = self.sender.send(DriverEvent::Error(format!("runtime command acceptance or outcome is uncertain ({stage:?}): {detail}")));
                    }
                    _ => {}
                }
            }
            Err(error) => {
                let _ = self.sender.send(DriverEvent::Error(format!(
                    "runtime command failed: {error}"
                )));
            }
        }
    }

    fn submit(&self, input: Value) {
        let turn_id = Uuid::new_v4();
        *self.active_turn.lock().unwrap() = Some(turn_id);
        self.command(
            Uuid::new_v4(),
            CanonicalAction::Submit {
                delivery_id: Uuid::new_v4(),
                turn_id,
                input,
            },
        );
    }

    /// The forwarder consumed this link's applied marker: every host event
    /// through `sequence` is durably applied — record the receipt, then ack.
    fn note_applied(&self, cursor: &RuntimeEventCursor) {
        if cursor.epoch != self.marker_epoch || cursor.runtime_id != self.runtime_id {
            return;
        }
        let Ok(Some(mut row)) = self.store.stable_link(self.task_id) else {
            return;
        };
        if row.runtime_instance != self.instance
            || row.runtime_session_id != self.sref.session_id
            || row.generation != self.sref.generation
        {
            return;
        }
        if cursor.sequence > row.applied_seq {
            row.applied_seq = cursor.sequence;
            row.updated_at = crate::model::unix_time();
            if self.store.stable_link_save(&row).is_err() {
                return;
            }
        }
        let mut client = self.client.lock().unwrap();
        let _ = client.acknowledge(
            self.epoch,
            EventCursor {
                session_id: self.sref.session_id,
                generation: self.sref.generation,
                applied: cursor.sequence,
            },
        );
    }

    /// A forwarded agent request arrives on the service connection. The
    /// receipt dedups by request identity before the effect runs.
    fn serve_request(&self, request: &ForwardedRequest) -> ForwardedOutcome {
        if request.session != self.sref {
            return ForwardedOutcome::Failed {
                detail: "ingress session binding mismatch".into(),
            };
        }
        match self.store.stable_receipt(request.request_id) {
            Ok(Some(receipt)) => {
                if receipt.session_id != self.task_id || receipt.generation != self.sref.generation
                {
                    return ForwardedOutcome::Failed {
                        detail: "ingress receipt belongs to another owner".into(),
                    };
                }
                // Already attempted — replay the recorded outcome, never
                // the effect.
                return serde_json::from_value(receipt.outcome.clone()).unwrap_or(
                    ForwardedOutcome::Uncertain {
                        detail: "a recorded receipt could not be decoded".into(),
                    },
                );
            }
            Ok(None) => {}
            Err(error) => {
                return ForwardedOutcome::Uncertain {
                    detail: format!("the receipts store could not be read: {error}"),
                };
            }
        }
        // Persist acceptance BEFORE executing. A crash after an external
        // effect but before its result leaves uncertainty, never a new retry.
        let pending = ForwardedOutcome::Uncertain {
            detail: "daemon execution accepted; outcome requires reconciliation".into(),
        };
        let receipt = StableReceipt {
            request_id: request.request_id,
            session_id: self.task_id,
            generation: self.sref.generation,
            class: format!("{:?}", request.class).to_lowercase(),
            outcome: serde_json::to_value(&pending).unwrap_or(Value::Null),
            recorded_at: crate::model::unix_time(),
        };
        match self.store.stable_receipt_begin(&receipt) {
            Ok(true) => {}
            Ok(false) => return self.serve_request(request),
            Err(_) => {
                return ForwardedOutcome::Uncertain {
                    detail: "execution acceptance could not be persisted".into(),
                };
            }
        }
        let outcome = self.service.execute(request);
        let mut completed = receipt.clone();
        completed.outcome = serde_json::to_value(&outcome).unwrap_or(Value::Null);
        if self
            .store
            .stable_receipt_finish(&receipt, &completed)
            .is_err()
        {
            return pending;
        }
        outcome
    }
}

/// The event pump: replay from the durable cursor, translate, interleave
/// the applied marker the forwarder acks through `note_applied`.
fn spawn_pump(shared: &Arc<LinkShared>) {
    let worker = shared.clone();
    let thread = std::thread::Builder::new()
        .name("goddard-stable-pump".into())
        .spawn(move || pump_loop(worker))
        .expect("stable runtime pump spawns");
    shared.threads.lock().unwrap().push(thread);
}

fn pump_loop(shared: Arc<LinkShared>) {
    let mut cursor = shared.cursor();
    let mut native = crate::driver::NativeEventProjection::default();
    if let Ok(inventory) = shared.client.lock().unwrap().reconcile(shared.epoch) {
        if let Some(config) = inventory
            .sessions
            .iter()
            .find(|inv| inv.session == shared.sref)
            .and_then(|inv| inv.effective.as_ref())
        {
            native.set_codex_thread(
                config
                    .config
                    .options
                    .get("codexThreadId")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            );
        }
    }
    let mut conn: Option<RuntimeClient> = None;
    while !shared.stop.load(Ordering::Acquire) {
        if conn.is_none() {
            conn = RuntimeClient::controller(
                &shared.root,
                &shared.credential,
                &ClientHello::current(),
            )
            .ok()
            .map(|(client, _)| client);
            if conn.is_none() {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        }
        let client = conn.as_mut().unwrap();
        match client.events(shared.epoch, cursor, POLL_WAIT_MS) {
            Ok(page) => {
                let mut last = None;
                for env in page.events {
                    if env.sequence <= shared_cursor_applied(&shared) {
                        continue;
                    }
                    match &env.event {
                        ProviderEvent::TurnStarted { turn_id, .. } => {
                            *shared.active_turn.lock().unwrap() = Some(*turn_id);
                        }
                        ProviderEvent::TurnCompleted { turn_id, .. }
                        | ProviderEvent::TurnCancelled { turn_id } => {
                            let mut active = shared.active_turn.lock().unwrap();
                            if *active == Some(*turn_id) {
                                *active = None;
                            }
                        }
                        _ => {}
                    }
                    match &env.event {
                        ProviderEvent::ConfigActivated {
                            revision,
                            effective,
                            ..
                        } => {
                            let mut current = shared.config_revision.lock().unwrap();
                            *current = (*current).max(*revision);
                            native.set_codex_thread(
                                effective
                                    .options
                                    .get("codexThreadId")
                                    .and_then(Value::as_str)
                                    .map(str::to_owned),
                            );
                        }
                        ProviderEvent::RequestOpened {
                            request_id,
                            subject,
                            class,
                            ..
                        } => {
                            let event = native.request(shared.provider, request_id, subject).or_else(|| {
                                if *class == RequestClass::Tool {
                                    Some(DriverEvent::Error("provider tool is waiting for the unqualified product bridge".into()))
                                } else { translate(&env) }
                            });
                            if let Some(event) = event {
                                if shared.sender.send(event).is_err() {
                                    return;
                                }
                            }
                        }
                        ProviderEvent::TurnDelta { delta, .. } if !delta.is_string() => {
                            for event in native.project(shared.provider, delta) {
                                if shared.sender.send(event).is_err() {
                                    return;
                                }
                            }
                        }
                        _ => {
                            if let Some(event) = translate(&env) {
                                if shared.sender.send(event).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    last = Some(env.sequence);
                }
                if let Some(sequence) = last {
                    cursor.applied = sequence;
                    // The marker rides the same ordered channel — when the
                    // forwarder consumes it, every earlier event is durable.
                    let _ = shared.sender.send(DriverEvent::RuntimeEventCursorAdvanced(
                        RuntimeEventCursor {
                            runtime_id: shared.runtime_id,
                            epoch: shared.marker_epoch,
                            sequence,
                        },
                    ));
                }
            }
            Err(ClientError::Io(_)) => conn = None,
            Err(ClientError::Host(waku_runtime::contract::HostError::StaleEpoch { .. })) => {
                // A newer controller owns this host — the stale link is
                // replaced, not erroring the session.
                return;
            }
            Err(ClientError::Host(error)) => {
                let _ = shared.sender.send(DriverEvent::Error(format!(
                    "the runtime host refused event replay: {error}"
                )));
                let _ = shared.sender.send(DriverEvent::ProcessExited);
                return;
            }
            Err(ClientError::Protocol(detail)) => {
                let _ = shared.sender.send(DriverEvent::Error(format!(
                    "the runtime host's protocol broke: {detail}"
                )));
                let _ = shared.sender.send(DriverEvent::ProcessExited);
                return;
            }
        }
    }
}

fn shared_cursor_applied(shared: &Arc<LinkShared>) -> u64 {
    shared
        .store
        .stable_link(shared.task_id)
        .ok()
        .flatten()
        .map(|row| row.applied_seq)
        .unwrap_or(0)
}

/// The service pump: drains forwarded agent requests under this epoch and
/// reports each outcome once — deduped by receipt so re-drained requests
/// never re-execute.
fn spawn_service(shared: &Arc<LinkShared>) {
    let worker = shared.clone();
    let thread = std::thread::Builder::new()
        .name("goddard-stable-service".into())
        .spawn(move || service_loop(worker))
        .expect("stable runtime service spawns");
    shared.threads.lock().unwrap().push(thread);
}

fn service_loop(shared: Arc<LinkShared>) {
    let mut conn: Option<RuntimeClient> = None;
    while !shared.stop.load(Ordering::Acquire) {
        if conn.is_none() {
            conn = RuntimeClient::service(
                &shared.root,
                &shared.credential,
                shared.epoch,
                &ClientHello::current(),
            )
            .ok()
            .map(|(client, _)| client);
            if conn.is_none() {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        }
        let client = conn.as_mut().unwrap();
        match client.service_drain(shared.epoch, POLL_WAIT_MS) {
            Ok(requests) => {
                for request in requests {
                    let _service_guard = shared.controller.service_gate.lock().unwrap();
                    // A second connection can already have drained this request.
                    // Verify current control before starting any new execution.
                    if shared
                        .client
                        .lock()
                        .unwrap()
                        .reconcile(shared.epoch)
                        .is_err()
                    {
                        return;
                    }
                    let target = shared
                        .controller
                        .routes
                        .lock()
                        .unwrap()
                        .get(&request.session.session_id)
                        .and_then(Weak::upgrade);
                    let Some(target) = target.filter(|target| {
                        target.sref == request.session && !target.stop.load(Ordering::Acquire)
                    }) else {
                        // A survivor not yet attached must wait, not execute as
                        // an unrelated task or be marked failed.
                        continue;
                    };
                    let outcome = target.serve_request(&request);
                    if let Err(error) =
                        client.service_result(shared.epoch, request.request_id, outcome)
                    {
                        // The receipt is durable — the next drain answers
                        // from it rather than re-executing.
                        eprintln!("goddard-daemon: could not report an ingress outcome: {error}");
                    }
                }
            }
            Err(ClientError::Io(_)) => conn = None,
            Err(ClientError::Host(waku_runtime::contract::HostError::StaleEpoch { .. })) => {
                return;
            }
            Err(_) => return,
        }
    }
}

impl StableLink {
    /// The forwarder's callback on this link's applied marker: every host
    /// event through `cursor.sequence` is durably applied — record the
    /// receipt, then acknowledge.
    pub(crate) fn note_applied(&self, cursor: &RuntimeEventCursor) {
        self.shared.note_applied(cursor);
    }

    /// Test-visible handle to the receipt-deduping ingress path.
    #[cfg(test)]
    pub(crate) fn serve_request(&self, request: &ForwardedRequest) -> ForwardedOutcome {
        self.shared.serve_request(request)
    }

    /// The daemon issues scoped agent grants through its own link — the
    /// controller authority that owns the session names which agents may
    /// address it.
    #[cfg(test)]
    pub(crate) fn issue_agent_grant(
        &self,
        ttl_ms: u64,
    ) -> Result<waku_runtime::contract::SessionGrant, ClientError> {
        self.shared
            .client
            .lock()
            .unwrap()
            .issue_grant(self.shared.epoch, self.shared.sref, ttl_ms)
    }

    #[cfg(test)]
    pub(crate) fn sref(&self) -> SessionRef {
        self.shared.sref
    }
}

impl DriverControl for StableLink {
    fn prompt(&self, prompt: String) {
        self.shared.submit(Value::String(prompt));
    }

    fn prompt_with_attachments(
        &self,
        prompt: String,
        attachments: Vec<crate::model::MessageAttachment>,
    ) {
        if attachments.is_empty() {
            self.prompt(prompt);
        } else {
            let _ = self.shared.sender.send(DriverEvent::Error(
                "attachments are not supported by the stable runtime adapters".into(),
            ));
        }
    }

    fn supports_steer(&self) -> bool {
        false
    }

    fn steer(&self, prompt: String) {
        let _ = self.shared.sender.send(DriverEvent::SteerRejected {
            message: prompt,
            reason: "the stable runtime adapter cannot steer an executing turn".into(),
            reason_i18n: None,
            hidden: false,
        });
    }

    fn cancel(&self) {
        let turn = self.shared.active_turn.lock().unwrap().take();
        if let Some(turn_id) = turn {
            self.shared
                .command(Uuid::new_v4(), CanonicalAction::CancelTurn { turn_id });
        }
    }

    /// Detach — never teardown. The runtime session is meant to outlive the
    /// daemon's entry; explicit termination belongs to
    /// `delete_provider_session` and the host's own drain.
    fn begin_shutdown(&self) {}

    fn respond(&self, request_id: String, option_id: String) {
        self.shared.command(
            Uuid::new_v4(),
            CanonicalAction::ResolveRequest {
                request_id,
                resolution: match self.shared.provider {
                    ProviderKind::Codex => json!({ "decision": option_id }),
                    _ => json!({ "optionId": option_id }),
                },
            },
        );
    }

    fn respond_user_input(&self, request_id: String, answers: Vec<crate::model::UserInputAnswer>) {
        self.shared.command(
            Uuid::new_v4(),
            CanonicalAction::ResolveRequest {
                request_id,
                resolution: match self.shared.provider {
                    ProviderKind::Codex => {
                        let values: serde_json::Map<String, Value> = answers
                            .into_iter()
                            .map(|answer| (answer.question_id, json!({"answers": answer.answers})))
                            .collect();
                        json!({"answers": values})
                    }
                    _ => serde_json::to_value(&answers).unwrap_or(Value::Null),
                },
            },
        );
    }

    fn apply_options(&self, options: crate::driver::SessionOptions) -> bool {
        if options.mode != self.shared.mode
            || options.service_tier.is_some()
            || options.context_window.is_some()
        {
            return false;
        }
        let mut revision = self.shared.config_revision.lock().unwrap();
        let Some(next) = revision.checked_add(1) else {
            return false;
        };
        let action = CanonicalAction::Reconfigure {
            revision: next,
            target: waku_runtime::contract::RequestedConfig {
                provider: Some(self.shared.provider.id().into()),
                model: options.model.clone(),
                effort: options.reasoning_effort.clone(),
                options: Value::Null,
            },
        };
        let mut client = self.shared.client.lock().unwrap();
        let applied = execute_safe(&mut client, &self.shared.envelope(Uuid::new_v4(), action))
            .map(|receipt| {
                matches!(
                    receipt.disposition,
                    waku_runtime::contract::CommandDisposition::ProviderAccepted
                        | waku_runtime::contract::CommandDisposition::Terminal {
                            outcome: waku_runtime::contract::TerminalOutcome::Completed,
                            ..
                        }
                )
            })
            .unwrap_or(false);
        if applied {
            *revision = next;
        }
        applied
    }

    /// The only scoped teardown the stable path performs: fence the host
    /// generation, release the link's durable claims, and clear the task's
    /// link row so a later attempt starts clean.
    fn delete_provider_session(&self) {
        let mut client = self.shared.client.lock().unwrap();
        let receipt = execute_safe(
            &mut client,
            &self
                .shared
                .envelope(Uuid::new_v4(), CanonicalAction::StopSession),
        );
        if matches!(
            receipt,
            Ok(waku_runtime::contract::CommandReceipt {
                disposition: waku_runtime::contract::CommandDisposition::Terminal {
                    outcome: waku_runtime::contract::TerminalOutcome::Completed,
                    ..
                },
                ..
            })
        ) {
            let _ = self.shared.store.stable_link_clear(self.shared.task_id);
        } else {
            let _ = self.shared.sender.send(DriverEvent::Error(
                "stop pending — provider termination is not confirmed".into(),
            ));
        }
    }

    fn rollback(
        &self,
        _turns: usize,
    ) -> anyhow::Result<Option<crate::model::ProviderResumeCursor>> {
        bail!("rollback is not supported by the stable provider runtime")
    }
}

fn link_error(error: ClientError) -> anyhow::Error {
    anyhow!("{error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::time::Instant;
    use waku_runtime::adapter::AdapterFactory;
    use waku_runtime::contract::{IngressDisposition, IngressKind, IngressRequest};
    use waku_runtime::fake::FakeFactory;
    use waku_runtime::host::{Budgets, HostOptions, RuntimeHost, SystemClock};
    use waku_runtime::registry::AdapterRegistry;
    use waku_runtime::server::RuntimeServer;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("waku-stable-{tag}-{}", Uuid::new_v4().simple()));
        waku_runtime::ipc::private_root(&root).unwrap();
        root
    }

    struct Rig {
        runtime_root: PathBuf,
        backend: Arc<WakuBackend>,
        session_id: Uuid,
        factory: FakeFactory,
        controller: String,
        _server: RuntimeServer,
    }

    /// One test backend plus one host+server at an isolated runtime root;
    /// `adapters` names what the registry can serve — `provider.id()` must
    /// be among them for admissions to land.
    fn rig(adapters: &[&str]) -> Rig {
        let runtime_root = temp_root("runtime");
        let factory = FakeFactory::new();
        let mut registry = AdapterRegistry::new();
        for name in adapters {
            registry.register(
                *name,
                Arc::new(factory.clone()) as Arc<dyn AdapterFactory>,
            );
        }
        let host = RuntimeHost::open(
            &runtime_root,
            HostOptions {
                namespace: STABLE_NAMESPACE.into(),
                budgets: Budgets::default(),
                journal_fault: None,
            },
            Arc::new(registry),
            Box::new(SystemClock),
        )
        .expect("host opens");
        let controller = host.credentials().controller.clone();
        let server = RuntimeServer::bind(host, &runtime_root, Duration::from_millis(5))
            .expect("server binds");
        let (backend, session_id) =
            crate::daemon::tests::surface_test_backend(&temp_root("backend"));
        let backend = Arc::new(backend);
        backend.install_weak_self();
        Rig {
            runtime_root,
            backend,
            session_id,
            factory,
            controller,
            _server: server,
        }
    }

    fn options() -> DriverStartOptions {
        DriverStartOptions {
            binary: PathBuf::from("unused"),
            cwd: PathBuf::from("."),
            mode: crate::model::RuntimeMode::default(),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_window: None,
            agent_preset: None,
            computer_use_enabled: false,
            agent: None,
            read_own_transcript: false,
            subagents: None,
            computer_use_runtime: None,
            mcp_servers: Vec::new(),
            http_mcp_capability_recorder: None,
            provider_cursor: None,
            eval: None,
            sandbox: None,
            allow_model_fallback: false,
            ephemeral: false,
        }
    }

    fn link(
        rig: &Rig,
        runtime_id: Uuid,
        service: Option<Arc<dyn IngressService>>,
    ) -> (
        DriverHandle,
        StableLink,
        crossbeam_channel::Receiver<DriverEvent>,
    ) {
        let (sender, rx) = crate::driver::test_event_channel();
        let (handle, link) = start_link(
            &rig.backend,
            rig.backend.task_store(),
            rig.session_id,
            runtime_id,
            ProviderKind::Codex,
            &options(),
            sender,
            &rig.runtime_root,
            service,
        )
        .expect("link attaches");
        (handle, link, rx)
    }

    /// A controller client for the host, independent of any link.
    fn controller(rig: &Rig) -> RuntimeClient {
        RuntimeClient::controller(
            &rig.runtime_root,
            &rig.controller,
            &ClientHello::current(),
        )
        .expect("controller connects")
        .0
    }

    /// An admission wires the link row, pumps host events into the driver
    /// channel in order, and interleaves the applied marker the forwarder
    /// acks through.
    #[test]
    fn stable_link_pumps_events_with_applied_markers() {
        let rig = rig(&["codex"]);
        let runtime_id = Uuid::new_v4();
        let (handle, link, rx) = link(&rig, runtime_id, None);

        let row = rig
            .backend
            .task_store()
            .stable_link(rig.session_id)
            .unwrap()
            .expect("link row recorded");
        assert_eq!(row.runtime_session_id, runtime_id);
        assert_eq!(row.generation, 1);
        assert_eq!(row.status, "live");

        let driver = rig
            .factory
            .for_session(link.sref().session_id)
            .expect("adapter attached");
        driver.emit(ProviderEvent::SessionLive);
        driver.emit(ProviderEvent::TurnDelta {
            turn_id: Uuid::new_v4(),
            delta: json!("hello"),
        });

        // The pump translates the events in order and closes the page with
        // a cursor marker.
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        let marker = loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(event) => {
                    if let DriverEvent::RuntimeEventCursorAdvanced(cursor) = &event {
                        break *cursor;
                    }
                    seen.push(event);
                }
                Err(_) => assert!(Instant::now() < deadline, "no marker arrived"),
            }
        };
        assert!(matches!(seen[0], DriverEvent::Connected { .. }));
        assert!(matches!(seen[1], DriverEvent::TextDelta(ref text) if text == "hello"));
        assert!(marker.sequence >= 2);

        // The forwarder-side callback: durable applied_seq, then host ack.
        link.note_applied(&marker);
        let row = rig
            .backend
            .task_store()
            .stable_link(rig.session_id)
            .unwrap()
            .unwrap();
        assert_eq!(row.applied_seq, marker.sequence);
        let mut daemon = controller(&rig);
        let grant = daemon.acquire_control(&ClientHello::current()).unwrap();
        let inventory = daemon.reconcile(grant.epoch).unwrap();
        let inv = inventory
            .sessions
            .iter()
            .find(|inv| inv.session == link.sref())
            .expect("session in inventory");
        assert_eq!(inv.acknowledged, marker.sequence);
        drop(handle);
    }

    /// A replacement daemon attempt attaches to the surviving host session
    /// — the link row still names it live — instead of admitting again.
    #[test]
    fn stable_link_reattaches_to_surviving_session() {
        let rig = rig(&["codex"]);
        let first_attempt = Uuid::new_v4();
        let (_h1, link1, _rx1) = link(&rig, first_attempt, None);
        let sref = link1.sref();
        assert_eq!(sref.session_id, first_attempt);

        // The first daemon's link goes away — the host session survives.
        drop(link1);

        let second_attempt = Uuid::new_v4();
        let (_h2, link2, _rx2) = link(&rig, second_attempt, None);
        // The new attempt carries the old runtime session's work.
        assert_eq!(link2.sref(), sref);
        let row = rig
            .backend
            .task_store()
            .stable_link(rig.session_id)
            .unwrap()
            .unwrap();
        assert_eq!(row.runtime_session_id, first_attempt);
    }

    /// A second attachment replays only what the first never applied —
    /// the durable cursor fences already-seen sequences.
    #[test]
    fn stable_link_replays_only_unapplied_events() {
        let rig = rig(&["codex"]);
        let (_h1, link1, rx1) = link(&rig, Uuid::new_v4(), None);
        let sref = link1.sref();
        let driver = rig.factory.for_session(sref.session_id).unwrap();
        driver.emit(ProviderEvent::SessionLive);

        // Apply the live marker through the link's ack path.
        let marker = loop {
            if let Ok(DriverEvent::RuntimeEventCursorAdvanced(cursor)) =
                rx1.recv_timeout(Duration::from_secs(10))
            {
                break cursor;
            }
        };
        link1.note_applied(&marker);
        drop(link1);

        // Two more events land while no daemon watches.
        let turn = Uuid::new_v4();
        driver.emit(ProviderEvent::TurnStarted {
            turn_id: turn,
            delivery_id: Uuid::new_v4(),
        });
        driver.emit(ProviderEvent::TurnDelta {
            turn_id: turn,
            delta: json!("late text"),
        });
        drop(driver);

        let (_h2, _link2, rx2) = link(&rig, Uuid::new_v4(), None);
        // The replay page contains only the unapplied tail.
        let mut translated = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match rx2.recv_timeout(Duration::from_millis(50)) {
                Ok(DriverEvent::RuntimeEventCursorAdvanced(_)) => break,
                Ok(event) => translated.push(event),
                Err(_) => assert!(Instant::now() < deadline, "no second marker"),
            }
        }
        assert_eq!(translated.len(), 2);
        assert!(matches!(translated[0], DriverEvent::TurnStarted));
        assert!(matches!(
            translated[1],
            DriverEvent::TextDelta(ref text) if text == "late text"
        ));
    }

    /// Forwarded agent requests execute once: the durable receipt answers
    /// a re-presented request identity, never the effect again.
    #[test]
    fn stable_link_ingress_dedups_effects_by_receipt() {
        struct Counting(AtomicUsize);
        impl IngressService for Counting {
            fn execute(&self, _request: &ForwardedRequest) -> ForwardedOutcome {
                self.0.fetch_add(1, AtomicOrdering::SeqCst);
                ForwardedOutcome::Completed {
                    result: json!({ "answer": 42 }),
                }
            }
        }
        let rig = rig(&["codex"]);
        let counting = Arc::new(Counting(AtomicUsize::new(0)));
        let (_handle, link, _rx) = link(&rig, Uuid::new_v4(), Some(counting.clone()));

        let request = ForwardedRequest {
            request_id: Uuid::new_v4(),
            session: link.sref(),
            grant_id: Uuid::new_v4(),
            class: IngressKind::Cli,
            subject: json!({ "command": "boss status" }),
            deadline_ms: u64::MAX / 2,
        };
        let first = link.serve_request(&request);
        let second = link.serve_request(&request);
        assert_eq!(first, second);
        assert_eq!(counting.0.load(AtomicOrdering::SeqCst), 1);
        assert!(rig
            .backend
            .task_store()
            .stable_receipt(request.request_id)
            .unwrap()
            .is_some());
    }

    /// The runtime-to-daemon ingress round-trip over real sockets: a scoped
    /// agent request drains through the link's service connection and its
    /// recorded outcome is the truth the agent reads back.
    #[test]
    fn stable_link_services_agent_ingress_over_ipc() {
        let rig = rig(&["codex"]);
        let (_handle, link, _rx) = link(&rig, Uuid::new_v4(), None);

        // The link issues the agent grant under its own controller epoch —
        // a second `acquire_control` would fence it.
        let agent_grant = link.issue_agent_grant(120_000).unwrap();
        let mut agent = RuntimeClient::agent(&rig.runtime_root, &agent_grant).unwrap();

        let request_id = Uuid::new_v4();
        agent
            .ingress(IngressRequest {
                request_id,
                class: IngressKind::Cli,
                // The link's own executor refuses an unscoped command
                // honestly rather than dispatching raw text.
                subject: json!({ "bogus": true }),
                deadline_ms: u64::MAX / 2,
            })
            .unwrap();

        // The link's service thread drains, executes, and resolves — the
        // agent waits for the recorded truth.
        let record = agent.ingress_wait(request_id, 10_000).unwrap();
        assert!(matches!(
            record.disposition,
            IngressDisposition::Failed { .. } | IngressDisposition::Completed { .. }
        ));
    }

    /// An unregistered adapter fails the admission loudly — the opt-in
    /// never silently falls back to a legacy driver.
    #[test]
    fn stable_link_fails_loudly_without_adapter() {
        let rig = rig(&[]);
        let (sender, _rx) = crate::driver::test_event_channel();
        let result = start_link(
            &rig.backend,
            rig.backend.task_store(),
            rig.session_id,
            Uuid::new_v4(),
            ProviderKind::Codex,
            &options(),
            sender,
            &rig.runtime_root,
            None,
        );
        assert!(result.is_err(), "an unknown adapter must fail admission");
    }

    /// A replaced controller's epoch can no longer command the host — the
    /// link surfaces the fence as an error event, not a silent drop.
    #[test]
    fn stable_link_fenced_epoch_cannot_command() {
        let rig = rig(&["codex"]);
        let (_handle, _link, rx) = link(&rig, Uuid::new_v4(), None);

        // A newer controller takes over — the link's epoch goes stale.
        let mut daemon = controller(&rig);
        daemon.acquire_control(&ClientHello::current()).unwrap();

        // The next command is fenced; the link surfaces it as an error.
        _handle.prompt("after the fence".into());
        let deadline = Instant::now() + Duration::from_secs(10);
        let saw_error = loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(DriverEvent::Error(_)) => break true,
                Ok(_) => continue,
                Err(_) => assert!(Instant::now() < deadline, "no fence error arrived"),
            }
        };
        assert!(saw_error);
    }

    /// Spin until `check` reports true, bounded — a test failure waits the
    /// deadline out rather than hanging.
    fn until(what: &str, mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !check() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The whole Stage-2C chain end to end: a cold `ensure_agent_runtime`
    /// attaches through the host, the forwarder applies events and acks
    /// durably, and a wholesale replacement daemon — a second backend over
    /// the same `app.db` — attaches to the surviving session rather than
    /// admitting again.
    #[test]
    fn daemon_replacement_reattaches_end_to_end() {
        let runtime_root = temp_root("runtime");
        let backend_root = temp_root("backend");
        let factory = FakeFactory::new();
        let mut registry = AdapterRegistry::new();
        registry.register(
            "codex",
            Arc::new(factory.clone()) as Arc<dyn AdapterFactory>,
        );
        let host = RuntimeHost::open(
            &runtime_root,
            HostOptions {
                namespace: STABLE_NAMESPACE.into(),
                budgets: Budgets::default(),
                journal_fault: None,
            },
            Arc::new(registry),
            Box::new(SystemClock),
        )
        .expect("host opens");
        let _server = RuntimeServer::bind(host, &runtime_root, Duration::from_millis(5))
            .expect("server binds");

        // The provider binary override keeps cold launches off the host's
        // real toolchain; the stable adapter ignores the path.
        let binary = backend_root.join("controlled-provider");
        std::fs::create_dir_all(&backend_root).unwrap();
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        // Seed the override into the settings file so the backend's own
        // store observes it at open.
        {
            let settings = crate::DaemonSettingsStore::open(backend_root.join("settings.json"))
                .unwrap();
            let mut document = settings.get();
            document.agent_tools_enabled = false;
            document.agent_settings_enabled = false;
            document.subagents_enabled = false;
            document.provider_binary_overrides.insert(
                ProviderKind::Codex,
                binary.display().to_string(),
            );
            settings.replace(document).unwrap();
        }
        let (mut backend, session_id) =
            crate::daemon::tests::surface_test_backend(&backend_root);
        backend.set_daemon_address("http://127.0.0.1:1".into());
        backend.stable_test_root = Some(runtime_root.clone());
        let backend = Arc::new(backend);
        backend.install_weak_self();
        let events = crate::EventSink::detached();

        // First daemon: admit.
        let (attempt1, _handle) = backend
            .ensure_agent_runtime(session_id, &events)
            .expect("cold runtime attaches");
        let row = backend
            .task_store()
            .stable_link(session_id)
            .unwrap()
            .expect("link row");
        assert_eq!(row.runtime_session_id, attempt1);

        // Provider events travel host → pump → forwarder → durable apply →
        // marker → host ack.
        let adapter = factory.for_session(attempt1).expect("adapter attached");
        adapter.emit_live();
        let turn = Uuid::new_v4();
        adapter.emit(ProviderEvent::TurnStarted {
            turn_id: turn,
            delivery_id: Uuid::new_v4(),
        });
        adapter.emit_delta(turn, "body");
        adapter.emit_completed(turn);
        until("the applied cursor to reach the turn's events", || {
            backend
                .task_store()
                .stable_link(session_id)
                .ok()
                .flatten()
                .is_some_and(|row| row.applied_seq >= 4)
        });

        // The daemon is replaced wholesale — the host session survives.
        drop(backend);
        let settings = crate::DaemonSettingsStore::open(backend_root.join("settings.json"))
            .unwrap();
        let mut successor = WakuBackend::new(
            settings,
            crate::persistence::StateStore::daemon(backend_root.join("app.db")),
        )
        .unwrap();
        successor.stable_test_root = Some(runtime_root.clone());
        successor.set_daemon_address("http://127.0.0.1:1".into());
        let successor = Arc::new(successor);
        successor.install_weak_self();

        let (attempt2, _handle2) = successor
            .ensure_agent_runtime(session_id, &events)
            .expect("successor attaches");
        let row = successor
            .task_store()
            .stable_link(session_id)
            .unwrap()
            .unwrap();
        // Attach, not resume: a new daemon attempt rides the surviving
        // runtime session — nothing prompts the provider again.
        assert_ne!(attempt2, attempt1);
        assert_eq!(row.runtime_session_id, attempt1);
        assert!(row.applied_seq >= 4);

        // New provider activity replays to the successor's forwarder only
        // once — the cursor resumes where the last daemon left it.
        let turn2 = Uuid::new_v4();
        adapter.emit(ProviderEvent::TurnStarted {
            turn_id: turn2,
            delivery_id: Uuid::new_v4(),
        });
        adapter.emit_delta(turn2, "after the replacement");
        until("the successor's cursor to advance", || {
            successor
                .task_store()
                .stable_link(session_id)
                .ok()
                .flatten()
                .is_some_and(|row| row.applied_seq >= 6)
        });
    }

    /// The opt-in gate: without a root there is no seam; a named root opts
    /// the provider in.
    #[test]
    fn stable_runtime_root_opt_in_gate() {
        let data_dir = PathBuf::from("/tmp/unused-data-dir");
        assert!(enabled(ProviderKind::Codex, &data_dir, None).is_none());
        let root = PathBuf::from("/tmp/unused-runtime-root");
        assert_eq!(
            enabled(ProviderKind::Codex, &data_dir, Some(&root)),
            Some(root)
        );
    }
    #[test]
    fn stable_links_share_control_and_route_ingress_to_its_owner() {
        struct OwnerService(Uuid, AtomicUsize);
        impl IngressService for OwnerService {
            fn execute(&self, request: &ForwardedRequest) -> ForwardedOutcome {
                assert_eq!(request.session.session_id, self.0);
                self.1.fetch_add(1, AtomicOrdering::SeqCst);
                ForwardedOutcome::Completed { result: json!(self.0) }
            }
        }
        let rig = rig(&["codex"]);
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();
        let first_service = Arc::new(OwnerService(first_id, AtomicUsize::new(0)));
        let second_service = Arc::new(OwnerService(second_id, AtomicUsize::new(0)));
        let (first_handle, first, first_rx) = link(&rig, first_id, Some(first_service.clone()));
        let second_task = Uuid::new_v4();
        let (sender, second_rx) = crate::driver::test_event_channel();
        let (second_handle, second) = start_link(&rig.backend, rig.backend.task_store(), second_task,
            second_id, ProviderKind::Codex, &options(), sender, &rig.runtime_root, Some(second_service.clone())).unwrap();
        assert_eq!(first.shared.epoch, second.shared.epoch);
        first_handle.prompt("one".into());
        second_handle.prompt("two".into());
        assert_eq!(rig.factory.for_session(first_id).unwrap().dispatched().len(), 1);
        assert_eq!(rig.factory.for_session(second_id).unwrap().dispatched().len(), 1);
        for (link, expected) in [(&first, first_id), (&second, second_id)] {
            let grant = link.issue_agent_grant(120_000).unwrap();
            let mut agent = RuntimeClient::agent(&rig.runtime_root, &grant).unwrap();
            let id = Uuid::new_v4();
            agent.ingress(IngressRequest { request_id: id, class: IngressKind::Cli,
                subject: json!({"test": true}), deadline_ms: u64::MAX / 2 }).unwrap();
            let record = agent.ingress_wait(id, 10_000).unwrap();
            assert!(matches!(record.disposition, IngressDisposition::Completed { result } if result == json!(expected)));
        }
        assert_eq!(first_service.1.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(second_service.1.load(AtomicOrdering::SeqCst), 1);
        assert!(!first_rx.try_iter().any(|event| matches!(event, DriverEvent::Error(_))));
        assert!(!second_rx.try_iter().any(|event| matches!(event, DriverEvent::Error(_))));
    }

    #[test]
    fn stable_ingress_effect_without_result_receipt_is_not_reexecuted() {
        struct LostResult { store: Arc<StateStore>, count: AtomicUsize }
        impl IngressService for LostResult {
            fn execute(&self, _request: &ForwardedRequest) -> ForwardedOutcome {
                self.count.fetch_add(1, AtomicOrdering::SeqCst);
                // Lose persistence after the downstream operation succeeded.
                rusqlite::Connection::open(self.store.path()).unwrap().execute_batch(
                    "CREATE TRIGGER fail_runtime_result BEFORE UPDATE ON stable_runtime_receipts
                     BEGIN SELECT RAISE(FAIL, 'injected receipt write failure'); END;"
                ).unwrap();
                ForwardedOutcome::Completed { result: json!("external effect succeeded") }
            }
        }
        let rig = rig(&["codex"]);
        let service = Arc::new(LostResult { store: rig.backend.task_store().clone(), count: AtomicUsize::new(0) });
        let (_handle, original, _rx) = link(&rig, Uuid::new_v4(), Some(service.clone()));
        let request = ForwardedRequest { request_id: Uuid::new_v4(), session: original.sref(), grant_id: Uuid::new_v4(),
            class: IngressKind::Cli, subject: json!("downstream operation"), deadline_ms: u64::MAX / 2 };
        assert!(matches!(original.serve_request(&request), ForwardedOutcome::Uncertain { .. }));
        assert!(matches!(original.serve_request(&request), ForwardedOutcome::Uncertain { .. }));
        assert_eq!(service.count.load(AtomicOrdering::SeqCst), 1);
        let mut foreign = request.clone();
        foreign.session.session_id = Uuid::new_v4();
        assert!(matches!(original.serve_request(&foreign), ForwardedOutcome::Failed { .. }));
        assert_eq!(service.count.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn stable_completion_uses_native_verdict() {
        for (result, success) in [(json!({"turn":{"status":"completed"}}), true),
            (json!({"turn":{"status":"failed","error":{"message":"provider failed"}}}), false),
            (json!({"stopReason":"end_turn"}), true), (json!({"stopReason":"max_tokens"}), false), (json!({}), false)] {
            let env = EventEnvelope { runtime_instance: Uuid::new_v4(), session_id: Uuid::new_v4(), generation: 1, sequence: 1, delivery_id: None, turn_id: None, request_id: None, config_revision: None,
                event: ProviderEvent::TurnCompleted { turn_id: Uuid::new_v4(), result } };
            assert!(matches!(translate(&env), Some(DriverEvent::TurnFinished { success: actual, .. }) if actual == success));
        }
    }

    #[test]
    fn stable_temporary_forwarder_clone_does_not_detach_driver() {
        let rig = rig(&["codex"]);
        let (handle, owner, _rx) = link(&rig, Uuid::new_v4(), None);
        drop(owner.clone());
        assert!(!owner.shared.stop.load(Ordering::Acquire));
        let shared = owner.shared.clone();
        drop(owner);
        assert!(!shared.stop.load(Ordering::Acquire));
        drop(handle);
        assert!(shared.stop.load(Ordering::Acquire));
    }

    #[test]
    fn stable_cursor_write_failure_keeps_host_history_unacknowledged() {
        let rig = rig(&["codex"]);
        let (_handle, link, rx) = link(&rig, Uuid::new_v4(), None);
        let adapter = rig.factory.for_session(link.sref().session_id).unwrap();
        adapter.emit_live();
        let marker = loop {
            if let DriverEvent::RuntimeEventCursorAdvanced(marker) = rx.recv_timeout(Duration::from_secs(10)).unwrap() { break marker; }
        };
        rusqlite::Connection::open(rig.backend.task_store().path()).unwrap().execute_batch(
            "CREATE TRIGGER fail_runtime_cursor BEFORE UPDATE ON stable_runtime_links
             BEGIN SELECT RAISE(FAIL, 'injected cursor write failure'); END;"
        ).unwrap();
        link.note_applied(&marker);
        let inventory = link.shared.client.lock().unwrap().reconcile(link.shared.epoch).unwrap();
        let session = inventory.sessions.iter().find(|session| session.session == link.sref()).unwrap();
        assert_eq!(session.acknowledged, 0);
        assert_eq!(rig.backend.task_store().stable_link(rig.session_id).unwrap().unwrap().applied_seq, 0);
    }

    #[test]
    fn stable_reattachment_cancels_the_original_open_turn() {
        let rig = rig(&["codex"]);
        let (handle, original, _rx) = link(&rig, Uuid::new_v4(), None);
        handle.prompt("original input".into());
        let turn = original.shared.active_turn.lock().unwrap().unwrap();
        let adapter = rig.factory.for_session(original.sref().session_id).unwrap();
        adapter.emit(ProviderEvent::TurnStarted { turn_id: turn, delivery_id: Uuid::new_v4() });
        // Reconcile drains the deterministic emitted event before replacement.
        original.shared.client.lock().unwrap().reconcile(original.shared.epoch).unwrap();
        let (successor, replacement, _rx2) = link(&rig, Uuid::new_v4(), None);
        assert_eq!(*replacement.shared.active_turn.lock().unwrap(), Some(turn));
        successor.cancel();
        assert!(adapter.cancels().contains(&turn));
    }

    #[test]
    fn stable_codex_launch_carries_policy_and_refuses_unsupported_features() {
        let mut opts = options();
        for (mode, policy, sandbox) in [
            (crate::model::RuntimeMode::Ask, "untrusted", "read-only"),
            (crate::model::RuntimeMode::AutoAcceptEdits, "on-request", "workspace-write"),
            (crate::model::RuntimeMode::FullAccess, "never", "danger-full-access"),
        ] {
            opts.mode = mode;
            let launch: waku_runtime_codex::CodexLaunch = serde_json::from_value(launch_payload(ProviderKind::Codex, &opts).unwrap()).unwrap();
            assert_eq!(launch.approval_policy, policy);
            assert_eq!(launch.sandbox, sandbox);
        }
        opts.mode = crate::model::RuntimeMode::Auto;
        assert!(launch_payload(ProviderKind::Codex, &opts).is_err());
        opts.mode = crate::model::RuntimeMode::Ask;
        opts.provider_cursor = Some(crate::model::ProviderResumeCursor::Codex { thread_id: "legacy".into() });
        assert!(launch_payload(ProviderKind::Codex, &opts).is_err());
    }

    /// Uses the production adapter over a controlled native RPC peer. This
    /// proves the daemon mapping, not qualification of a hosted provider.
    #[test]
    #[cfg(unix)]
    fn stable_codex_native_protocol_stream_and_approval_round_trip() {
        let runtime_root = temp_root("native-runtime");
        let provider_root = temp_root("native-provider");
        let binary = provider_root.join("controlled-codex");
        std::fs::write(&binary, r#"#!/usr/bin/env python3
import json,sys
def emit(v):
    print(json.dumps(v),flush=True)
for line in sys.stdin:
    v=json.loads(line)
    method=v.get('method')
    if method=='initialize':
        emit({'id':v['id'],'result':{}})
    elif method=='thread/start':
        assert v['params']['approvalPolicy']=='untrusted'
        assert v['params']['sandbox']=='read-only'
        emit({'id':v['id'],'result':{'thread':{'id':'owned-thread'},'model':'controlled','reasoningEffort':'low'}})
    elif method=='turn/start':
        assert v['params']['input']==[{'type':'text','text':'native input'}]
        emit({'id':v['id'],'result':{'turn':{'id':'owned-turn'}}})
        emit({'method':'turn/started','params':{'threadId':'owned-thread','turn':{'id':'owned-turn'}}})
        emit({'method':'item/agentMessage/delta','params':{'threadId':'owned-thread','turnId':'owned-turn','itemId':'message-1','delta':'native text'}})
        emit({'method':'item/reasoning/textDelta','params':{'threadId':'owned-thread','turnId':'owned-turn','itemId':'reasoning-1','delta':'native reasoning'}})
        emit({'id':'native-approval','method':'item/commandExecution/requestApproval','params':{'threadId':'owned-thread','turnId':'owned-turn','command':'controlled command'}})
    elif v.get('id')=='native-approval':
        assert v['result']=={'decision':'decline'}
        emit({'method':'turn/completed','params':{'threadId':'owned-thread','turn':{'id':'owned-turn','status':'failed','error':{'message':'controlled failure'}}}})
"#).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let host = RuntimeHost::open(&runtime_root,
            HostOptions { namespace: STABLE_NAMESPACE.into(), budgets: Budgets::default(), journal_fault: None },
            Arc::new(waku_runtime_codex::CodexFactory), Box::new(SystemClock)).unwrap();
        let server = RuntimeServer::bind(host, &runtime_root, Duration::from_millis(5)).unwrap();
        let (backend, task) = crate::daemon::tests::surface_test_backend(&temp_root("native-backend"));
        let backend = Arc::new(backend);
        backend.install_weak_self();
        let mut opts = options();
        opts.binary = binary;
        opts.cwd = provider_root.clone();
        opts.mode = crate::model::RuntimeMode::Ask;
        let (sender, rx) = crate::driver::test_event_channel();
        let (handle, link) = start_link(&backend, backend.task_store(), task, Uuid::new_v4(),
            ProviderKind::Codex, &opts, sender, &runtime_root, None).unwrap();
        handle.prompt("native input".into());
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut approval = false;
        let mut started = 0;
        loop {
            match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                DriverEvent::TurnStarted => started += 1,
                DriverEvent::TextDelta(delta) => text.push_str(&delta),
                DriverEvent::ReasoningDelta(delta) => reasoning.push_str(&delta),
                DriverEvent::Permission { request_id, options, .. } => {
                    assert_eq!(request_id, serde_json::to_string("native-approval").unwrap());
                    assert!(options.iter().any(|option| option.id == "decline" && !option.allow));
                    approval = true;
                    handle.respond(request_id, "decline".into());
                }
                DriverEvent::TurnFinished { success, summary, .. } => {
                    assert!(!success);
                    assert_eq!(summary.as_deref(), Some("controlled failure"));
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(started, 1);
        assert_eq!(text, "native text");
        assert_eq!(reasoning, "native reasoning");
        assert!(approval);
        handle.delete_provider_session();
        drop(handle);
        drop(link);
        drop(server);
        std::fs::remove_dir_all(provider_root).unwrap();
    }

}
