//! Scoped agent credentials and the daemon-side prompt queue behind them.
//!
//! When `agent_tools_enabled` is on, the daemon mints one bearer token per
//! provider runtime and hands it to the session's harness through the launch
//! environment (`GODDARD_AGENT_TOKEN`, see [`crate::driver`]). The token never
//! leaves daemon memory: it authenticates a WebSocket client exactly like the
//! master token but authorizes only `agentCreateSession` and `agentPrompt`,
//! and it dies with the runtime that carried it.
//!
//! Queue-mode prompts wait here rather than in the session's client-side
//! follow-up queue: delivery order is decided by the daemon, and the daemon
//! is also what marks each accepted prompt with the sending task's id so
//! agent-originated turns stay attributable in the transcript.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, anyhow};
use parking_lot::Mutex;
use subtle::ConstantTimeEq as _;
use uuid::Uuid;

use crate::model::DriverEvent;

/// A prompt an agent submitted, with the sending task it must be attributed
/// to. The same record tracks steer injections awaiting the provider's
/// `steerAccepted` echo so the mirrored message keeps its provenance.
pub struct AgentPrompt {
    /// The sender's own words — what the transcript and the mirrored queued
    /// chip show.
    pub prompt: String,
    /// The provider-facing text when a wrapper — the sender provenance
    /// envelope on a task-to-task message — replaced `prompt` on the wire.
    /// The provider's echo resolves the pending steer against this; `None`
    /// means `prompt` went out verbatim.
    pub transport: Option<String>,
    /// The task whose agent sent it. `None` is possible only for requests
    /// made with the master token, which carries no session scope.
    pub sender: Option<Uuid>,
    /// The [`crate::model::QueuedMessage`] mirroring this prompt in the
    /// session document, when one was written. The daemon owns that entry:
    /// it appears as a queued chip while parked and is removed when the
    /// prompt is confirmed delivered. `None` for prompts that never parked —
    /// steer-mode requests and queue prompts delivered immediately.
    pub queued_id: Option<Uuid>,
    /// Set when the steer is a daemon-injected context block rather than
    /// user or agent text: its `steerAccepted` echo stays out of the
    /// transcript and, for [`ContextSteer::Memory`], marks the session's
    /// memory injection delivered. A rejection leaves the session eligible
    /// so the next prompt retries.
    pub context: Option<ContextSteer>,
}

/// What accepting a hidden context steer settles for the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextSteer {
    /// Context blocks only — the echo confirms delivery.
    Blocks,
    /// The steer carried the project-memory block — accepting it marks the
    /// session's memory injection delivered.
    Memory,
}

/// Live turn bookkeeping the runtime event forwarder maintains per session.
#[derive(Clone, Copy, Default)]
struct AgentTurn {
    /// The provider reported `turnStarted` without a later `turnFinished`.
    open: bool,
    /// The provider is actively working inside the open turn. A parked turn
    /// stays open but idle: it accepts a message immediately.
    working: bool,
}

/// One runtime's agent-surface announcement: the scopes its launch env
/// granted and whether the session has already been told about them.
struct AgentSurface {
    scope: AgentSurfaceScope,
    announced: bool,
}

/// Shared agent surface state. One instance lives on the backend; the runtime
/// event forwarder holds a second reference so it can track turns, drain
/// queues, and attach provenance to echoed steers without round-tripping
/// through the request path.
#[derive(Default)]
pub struct AgentState {
    /// Scoped bearer token → the session owning the runtime it was minted
    /// for. Several tokens can name the same session when a task restarts.
    tokens: Mutex<HashMap<String, Uuid>>,
    /// The `goddard-agent` scopes each live runtime launched with, for
    /// sessions the provider did not tell about the CLI itself.
    surfaces: Mutex<HashMap<Uuid, AgentSurface>>,
    /// Queue-mode prompts waiting for the target session to become idle.
    queues: Mutex<HashMap<Uuid, VecDeque<AgentPrompt>>>,
    /// Steer injections in flight, oldest first.
    pending_steers: Mutex<HashMap<Uuid, VecDeque<AgentPrompt>>>,
    /// Sessions whose in-flight context steer carries the side-chat parent
    /// index — an accept marks it delivered, a rejection clears the flag so
    /// the next prompt retries.
    index_steers: Mutex<HashSet<Uuid>>,
    /// Sessions whose parent index a delivered steer or prepend already
    /// shipped — the index is a once-per-runtime snapshot, cleared when the
    /// runtime's credentials are revoked.
    parent_indexes: Mutex<HashSet<Uuid>>,
    turns: Mutex<HashMap<Uuid, AgentTurn>>,
}

impl AgentState {
    /// Mint a credential scoped to one provider session. The value is two
    /// UUIDs of entropy; it is stored, never derived, and never written down.
    pub fn mint(&self, session_id: Uuid) -> String {
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        self.tokens.lock().insert(token.clone(), session_id);
        token
    }

    /// Resolve a presented credential to its owning session. The comparison
    /// is constant-time over every minted token so probes cannot narrow the
    /// space by timing.
    pub fn resolve(&self, token: &str) -> Option<Uuid> {
        let candidate = token.as_bytes();
        self.tokens.lock().iter().find_map(|(known, session)| {
            bool::from(known.as_bytes().ct_eq(candidate)).then_some(*session)
        })
    }

    /// Drop every credential minted for the session's runtimes. Called when a
    /// runtime is closed, replaced, or reported exited — the token is valid
    /// only while the provider process that carries it lives. The surface
    /// announcement goes with it: a restarted runtime announces again to its
    /// fresh provider process.
    pub fn revoke_session(&self, session_id: Uuid) {
        self.tokens.lock().retain(|_, owner| *owner != session_id);
        self.surfaces.lock().remove(&session_id);
        // A revoked runtime loses everything its steers carried — the
        // restarted process gets the parent index again.
        self.index_steers.lock().remove(&session_id);
        self.parent_indexes.lock().remove(&session_id);
    }

    /// Whether the session's side-chat parent index is still undelivered.
    pub fn parent_index_owed(&self, session_id: Uuid) -> bool {
        !self.parent_indexes.lock().contains(&session_id)
    }

    /// The composed context steer carries the session's parent index — an
    /// accept settles it through [`Self::mark_parent_index_delivered`].
    pub fn note_index_steer(&self, session_id: Uuid) {
        self.index_steers.lock().insert(session_id);
    }

    /// An accepted context steer settles a pending index carry; a prepend
    /// path marks delivery directly since its prompt already shipped.
    pub fn mark_parent_index_delivered(&self, session_id: Uuid) {
        if self.index_steers.lock().remove(&session_id) {
            self.parent_indexes.lock().insert(session_id);
        }
    }

    /// The non-steer path's mark — the index rode the prompt itself.
    pub fn mark_parent_index_prepended(&self, session_id: Uuid) {
        self.parent_indexes.lock().insert(session_id);
    }

    /// Record the scopes a launch carried so first-prompt context can
    /// describe the tools the session actually has, for providers without a
    /// native announcement channel.
    pub fn note_surface(&self, session_id: Uuid, scope: AgentSurfaceScope) {
        self.surfaces.lock().insert(
            session_id,
            AgentSurface {
                scope,
                announced: false,
            },
        );
    }

    /// Whether this session's launch carried the `goddard-agent` env —
    /// pointer-style context only names the read surface when it exists.
    pub fn has_surface(&self, session_id: Uuid) -> bool {
        self.surfaces.lock().contains_key(&session_id)
    }

    /// The `goddard-agent` instruction a session is still owed — `None` when
    /// its runtime carries no surface or already heard about it. The steer
    /// path marks delivery on the provider's accept echo; a prompt-prepend
    /// caller marks it on send (`mark_surface_announced`).
    pub fn surface_block(&self, session_id: Uuid) -> Option<String> {
        let surfaces = self.surfaces.lock();
        let surface = surfaces.get(&session_id)?;
        if surface.announced {
            return None;
        }
        Some(surface_instruction("goddard-agent", &surface.scope))
    }

    /// The session was told about its agent surface — an injected context
    /// steer was accepted, or a prepended prompt shipped.
    pub fn mark_surface_announced(&self, session_id: Uuid) {
        if let Some(surface) = self.surfaces.lock().get_mut(&session_id) {
            surface.announced = true;
        }
    }

    /// Forget everything about a session that is gone for good.
    pub fn clear_session(&self, session_id: Uuid) {
        self.revoke_session(session_id);
        self.queues.lock().remove(&session_id);
        self.pending_steers.lock().remove(&session_id);
        self.turns.lock().remove(&session_id);
    }

    /// Forget every credential and queue. Called on daemon shutdown.
    pub fn clear(&self) {
        self.tokens.lock().clear();
        self.surfaces.lock().clear();
        self.queues.lock().clear();
        self.pending_steers.lock().clear();
        self.index_steers.lock().clear();
        self.parent_indexes.lock().clear();
        self.turns.lock().clear();
    }

    /// Update the session's turn bookkeeping from a runtime event. Returns
    /// the pending steer a rejection resolved, so the caller can tell a
    /// dropped context injection apart from a refused agent message.
    pub fn note_driver_event(&self, session_id: Uuid, event: &DriverEvent) -> Option<AgentPrompt> {
        match event {
            DriverEvent::TurnStarted => {
                *self.turns.lock().entry(session_id).or_default() = AgentTurn {
                    open: true,
                    working: true,
                };
            }
            DriverEvent::TurnParked => {
                if let Some(turn) = self.turns.lock().get_mut(&session_id) {
                    turn.working = false;
                }
            }
            DriverEvent::TurnFinished { .. } => {
                if let Some(turn) = self.turns.lock().get_mut(&session_id) {
                    turn.open = false;
                    turn.working = false;
                }
            }
            DriverEvent::ProcessExited => {
                self.turns.lock().remove(&session_id);
                // Steers can no longer be acknowledged; drop them so a
                // restarted runtime does not attribute an unrelated echo.
                self.pending_steers.lock().remove(&session_id);
                self.index_steers.lock().remove(&session_id);
            }
            DriverEvent::SteerRejected { message, .. } => {
                // A refused steer settles without delivering: the parent
                // index flag drops so the next prompt retries the carry.
                self.index_steers.lock().remove(&session_id);
                // A queue-drained prompt whose steer was refused goes back to
                // the head of the queue — its mirrored chip never left the
                // session, so the wait stays visible and ordered. A direct
                // steer-mode request has no queue entry and stays dropped.
                return match self.take_pending_steer(session_id, message) {
                    Some(entry) if entry.context.is_none() && entry.queued_id.is_some() => {
                        self.requeue_front(session_id, entry);
                        None
                    }
                    entry => entry,
                };
            }
            _ => {}
        }
        None
    }

    /// Whether the session has an open turn — running or parked. Steer-mode
    /// prompts require this.
    pub fn has_open_turn(&self, session_id: Uuid) -> bool {
        self.turns
            .lock()
            .get(&session_id)
            .is_some_and(|turn| turn.open)
    }

    /// Whether the provider is actively working a turn. Queue-mode prompts
    /// wait while this is true; a parked session (open but not working)
    /// accepts the next message right away.
    pub fn is_working(&self, session_id: Uuid) -> bool {
        self.turns
            .lock()
            .get(&session_id)
            .is_some_and(|turn| turn.open && turn.working)
    }

    /// Whether the session has an open turn that is not working — the state
    /// a queue-mode prompt may steer into without cutting a running turn
    /// short.
    pub fn has_parked_turn(&self, session_id: Uuid) -> bool {
        self.turns
            .lock()
            .get(&session_id)
            .is_some_and(|turn| turn.open && !turn.working)
    }

    /// Whether prompts are parked for this session awaiting delivery.
    pub fn has_queued(&self, session_id: Uuid) -> bool {
        self.queues
            .lock()
            .get(&session_id)
            .is_some_and(|queue| !queue.is_empty())
    }

    pub fn enqueue(&self, session_id: Uuid, prompt: AgentPrompt) {
        self.queues
            .lock()
            .entry(session_id)
            .or_default()
            .push_back(prompt);
    }

    /// Pop the next queued prompt in submission order.
    pub fn pop_queued(&self, session_id: Uuid) -> Option<AgentPrompt> {
        let mut queues = self.queues.lock();
        let queue = queues.get_mut(&session_id)?;
        let prompt = queue.pop_front();
        if queue.is_empty() {
            queues.remove(&session_id);
        }
        prompt
    }

    /// Put a popped prompt back at the head of the queue. A turn that
    /// started working mid-drain holds the remaining prompts until it
    /// finishes; submission order is preserved.
    pub fn requeue_front(&self, session_id: Uuid, prompt: AgentPrompt) {
        self.queues
            .lock()
            .entry(session_id)
            .or_default()
            .push_front(prompt);
    }

    /// Drop the queued prompt mirrored as `queued_id`, if it is still
    /// parked. Returns whether anything was removed — a `false` means the
    /// prompt already delivered (or this daemon restart lost the in-memory
    /// queue and only the session document's mirror remains).
    pub fn remove_queued(&self, session_id: Uuid, queued_id: Uuid) -> bool {
        let mut queues = self.queues.lock();
        let Some(queue) = queues.get_mut(&session_id) else {
            return false;
        };
        let before = queue.len();
        queue.retain(|prompt| prompt.queued_id != Some(queued_id));
        let removed = queue.len() != before;
        if queue.is_empty() {
            queues.remove(&session_id);
        }
        removed
    }

    /// Rebuild the in-memory queue from prompts the session document still
    /// mirrors as parked — a restart dropped the memory copy but not the
    /// persisted chips. Entries already in the queue win; `prompts` fills
    /// the gaps in document order.
    pub fn seed_queue(&self, session_id: Uuid, prompts: Vec<AgentPrompt>) {
        let mut queues = self.queues.lock();
        let queue = queues.entry(session_id).or_default();
        for prompt in prompts {
            if prompt
                .queued_id
                .is_some_and(|id| queue.iter().any(|queued| queued.queued_id == Some(id)))
            {
                continue;
            }
            queue.push_back(prompt);
        }
        if queue.is_empty() {
            queues.remove(&session_id);
        }
    }

    /// Whether a daemon context steer is still awaiting the provider's echo
    /// for this session — a second prompt must not compose another context
    /// injection while one is in flight.
    pub fn context_steer_pending(&self, session_id: Uuid) -> bool {
        self.pending_steers
            .lock()
            .get(&session_id)
            .is_some_and(|pending| pending.iter().any(|steer| steer.context.is_some()))
    }

    /// Remember a steer injection so the provider's `steerAccepted` echo can
    /// be attributed to its sender.
    pub fn record_pending_steer(&self, session_id: Uuid, prompt: AgentPrompt) {
        self.pending_steers
            .lock()
            .entry(session_id)
            .or_default()
            .push_back(prompt);
    }

    /// Resolve the steer the provider just accepted or rejected. Providers
    /// normally echo the transport text; a normalized echo still releases
    /// the oldest pending steer, matching how clients treat it.
    pub fn take_pending_steer(&self, session_id: Uuid, message: &str) -> Option<AgentPrompt> {
        let mut steers = self.pending_steers.lock();
        let pending = steers.get_mut(&session_id)?;
        let index = pending
            .iter()
            .position(|steer| steer.transport.as_deref().unwrap_or(&steer.prompt) == message)
            .unwrap_or(0);
        let prompt = pending.remove(index);
        if pending.is_empty() {
            steers.remove(&session_id);
        }
        prompt
    }
}

/// The agent surface one provider launch receives: the scoped credential,
/// the session it belongs to, the daemon's address, and where the
/// `goddard-agent` CLI lives. `DriverStartOptions` carries it to whichever
/// spawn path the provider uses.
#[derive(Clone, Debug)]
pub struct AgentLaunchEnv {
    pub token: String,
    /// The Waku task this runtime serves; the daemon attributes any prompt
    /// the token sends to it.
    pub task_id: Uuid,
    /// The task this session is a side chat of, when it is one. Delivered as
    /// `GODDARD_PARENT_TASK_ID` so the agent can find its parent's
    /// transcript without parsing it out of a prompt.
    pub parent_task_id: Option<Uuid>,
    /// The daemon's WebSocket address, as the daemon bound it.
    pub daemon_address: String,
    /// The `goddard-agent` executable.
    pub cli_path: PathBuf,
    /// A daemon-private directory for the session's CLI launcher. Providers
    /// whose sessions share one host process cannot receive per-session
    /// environment, so they write a credential-carrying shim here and point
    /// the session at it instead.
    pub shim_directory: PathBuf,
    /// Whether `goddard-agent create`/`prompt` — the opt-in cross-task surface —
    /// will accept calls on this credential.
    pub task_tools: bool,
    /// Whether the `goddard-agent command` settings writes will accept calls on
    /// this credential. On by default; off only when the user disabled the
    /// agent settings surface outright.
    pub settings_writes: bool,
}

impl AgentLaunchEnv {
    /// The scopes this launch carries, for composing the session's
    /// agent-surface instruction through whichever channel delivers it.
    pub fn scope(&self) -> AgentSurfaceScope {
        AgentSurfaceScope {
            task_tools: self.task_tools,
            settings_writes: self.settings_writes,
            parent_task_id: self.parent_task_id,
        }
    }
}

/// Which `goddard-agent` subcommands a session's credential will accept —
/// everything the agent-surface instruction needs that isn't the command
/// name itself.
#[derive(Clone, Copy, Debug)]
pub struct AgentSurfaceScope {
    pub task_tools: bool,
    pub settings_writes: bool,
    pub parent_task_id: Option<Uuid>,
}

/// Locate the `goddard-agent` binary to place on a provider's `PATH`.
///
/// Development and unpackaged installs keep it beside the daemon
/// executable; a packaged macOS app keeps it in `Contents/Resources` like
/// `goddard_js_repl`.
pub fn agent_cli_path() -> anyhow::Result<PathBuf> {
    let executable =
        std::env::current_exe().context("Goddard daemon executable path is unavailable")?;
    let name = if cfg!(windows) {
        "goddard-agent.exe"
    } else {
        "goddard-agent"
    };
    let bundled = executable
        .parent()
        .and_then(|macos| macos.parent())
        .map(|contents| contents.join("Resources").join(name));
    [Some(executable.with_file_name(name)), bundled]
        .into_iter()
        .flatten()
        .find(|path| path.is_file())
        .ok_or_else(|| anyhow!("the goddard-agent CLI is missing from this Goddard build"))
}

/// Write the session-scoped `goddard-agent` launcher shared-service providers
/// use. The shim bakes this session's credential into itself and execs the
/// real CLI, so a host process that serves many sessions never carries one
/// session's token in its own environment. Returns the shim's path — the
/// provider surfaces it to the agent through a session instruction.
pub fn write_session_shim(env: &AgentLaunchEnv) -> anyhow::Result<PathBuf> {
    crate::fs_ext::create_private_dir_all(&env.shim_directory)
        .with_context(|| format!("could not create {}", env.shim_directory.display()))?;
    #[cfg(unix)]
    let shim = {
        let path = env.shim_directory.join("goddard-agent");
        let parent = env
            .parent_task_id
            .map(|id| format!(" {}='{id}'", waku_protocol::AGENT_PARENT_TASK_ENV))
            .unwrap_or_default();
        let script = format!(
            "#!/bin/sh\nexec env {}='{}' {}='{}' {}='{}'{} '{}' \"$@\"\n",
            waku_protocol::DAEMON_ADDRESS_ENV,
            shell_quote_escape(&env.daemon_address),
            waku_protocol::AGENT_TOKEN_ENV,
            shell_quote_escape(&env.token),
            waku_protocol::AGENT_TASK_ENV,
            env.task_id,
            parent,
            shell_quote_escape(&env.cli_path.display().to_string()),
        );
        write_private_executable(&path, script.as_bytes())?;
        path
    };
    #[cfg(windows)]
    let shim = {
        let path = env.shim_directory.join("goddard-agent.cmd");
        let parent = env
            .parent_task_id
            .map(|id| format!("set \"{}={id}\"\r\n", waku_protocol::AGENT_PARENT_TASK_ENV))
            .unwrap_or_default();
        let script = format!(
            "@echo off\r\nset \"{}={}\"\r\nset \"{}={}\"\r\nset \"{}={}\"\r\n{}\"{}\" %*\r\n",
            waku_protocol::DAEMON_ADDRESS_ENV,
            env.daemon_address,
            waku_protocol::AGENT_TOKEN_ENV,
            env.token,
            waku_protocol::AGENT_TASK_ENV,
            env.task_id,
            parent,
            env.cli_path.display(),
        );
        write_private_executable(&path, script.as_bytes())?;
        path
    };
    Ok(shim)
}

/// The agent-surface instruction a session learns through whichever channel
/// its provider offers — an attached instruction entry, a launch flag, a
/// config-registered file, or the first-prompt context block. `command` is
/// how the session invokes the CLI: `goddard-agent` when `PATH` carries it,
/// the shim's path for shared-service providers.
pub fn surface_instruction(command: &str, scope: &AgentSurfaceScope) -> String {
    let mut instruction = format!(
        "<goddard-agent>\nGoddard gives this session a `{command}` CLI; \
         `{command} --help` documents every subcommand and its JSON payload."
    );
    if scope.task_tools {
        instruction.push_str(&format!(
            "\n- `create` — when the user asks you to create, start, or \
             spawn another task or session, including running work in a \
             separate task (e.g. `{command} create '{{\"prompt\": \"...\"}}')\n\
             - `prompt` — when the user asks you to send a message to \
             another task\n\
             - `read` — to read a task's transcript"
        ));
    } else {
        // Every scoped credential reads its own task's transcript — the
        // provider-switch handoff and side chats rely on it — so the read
        // bullet appears even when the cross-task surface is off.
        instruction.push_str(&format!(
            "\n- `read` — to read this task's transcript \
             (`{command} read '{{}}'`, or `'{{\"turn\": N}}'` for one turn)"
        ));
    }
    if scope.settings_writes {
        instruction.push_str(
            "\n- `command` — manage the user's custom commands; may be used \
             proactively whenever adding one would help",
        );
    }
    if scope.task_tools {
        instruction.push_str(
            "\n\n`create`, `prompt`, and `read` act on the user's other \
             tasks under this task's name — use them only when the user \
             asks, never for exploration, convenience, or \
             self-orchestration.",
        );
    }
    if let Some(parent) = scope.parent_task_id {
        instruction.push_str(&format!(
            " This session is a side chat of task {parent}; `read` its \
             transcript when you need its context."
        ));
    }
    instruction.push_str("\n</goddard-agent>");
    instruction
}

/// The prompt shared-service sessions get in place of a launch environment:
/// the same contract the CLI's own help states, with the session-scoped
/// launcher's path as the command because `PATH` cannot carry it.
pub fn shared_service_instruction(shim: &Path, env: &AgentLaunchEnv) -> String {
    surface_instruction(&shim.display().to_string(), &env.scope())
}

#[cfg(unix)]
fn shell_quote_escape(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn write_private_executable(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    use std::io::Write as _;
    let temporary = path.with_extension("tmp");
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .with_context(|| format!("could not create {}", temporary.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(std::fs::Permissions::from_mode(0o700))?;
        }
        file.write_all(contents)?;
    }
    std::fs::rename(&temporary, path)
        .with_context(|| format!("could not install {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(text: &str, sender: Option<Uuid>) -> AgentPrompt {
        AgentPrompt {
            prompt: text.to_owned(),
            transport: None,
            sender,
            queued_id: Some(Uuid::new_v4()),
            context: None,
        }
    }

    #[test]
    fn a_minted_token_resolves_to_its_owning_session_until_revoked() {
        let state = AgentState::default();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let first_token = state.mint(first);
        let second_token = state.mint(second);

        assert_eq!(state.resolve(&first_token), Some(first));
        assert_eq!(state.resolve(&second_token), Some(second));
        assert_eq!(state.resolve("not-a-token"), None);

        state.revoke_session(first);
        assert_eq!(state.resolve(&first_token), None);
        assert_eq!(state.resolve(&second_token), Some(second));
    }

    #[test]
    fn queued_prompts_pop_in_submission_order() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        state.enqueue(session, prompt("one", None));
        state.enqueue(session, prompt("two", None));

        assert_eq!(state.pop_queued(session).unwrap().prompt, "one");
        // A requeued prompt returns to the head ahead of later submissions.
        let popped = prompt("one", None);
        state.requeue_front(session, popped);
        state.enqueue(session, prompt("three", None));

        let order: Vec<String> = [
            state.pop_queued(session),
            state.pop_queued(session),
            state.pop_queued(session),
        ]
        .into_iter()
        .flatten()
        .map(|entry| entry.prompt)
        .collect();
        assert_eq!(order, ["one", "two", "three"]);
        assert!(state.pop_queued(session).is_none());
    }

    #[test]
    fn remove_queued_drops_only_the_named_entry() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        let first = prompt("one", None);
        let first_id = first.queued_id.unwrap();
        state.enqueue(session, first);
        state.enqueue(session, prompt("two", None));

        assert!(state.remove_queued(session, first_id));
        // A second removal finds nothing, and the sibling stays parked.
        assert!(!state.remove_queued(session, first_id));
        assert_eq!(state.pop_queued(session).unwrap().prompt, "two");
        assert!(state.pop_queued(session).is_none());
        // Removing from a session with no queue is a miss, not an error.
        assert!(!state.remove_queued(session, Uuid::new_v4()));
    }

    #[test]
    fn seed_queue_restores_mirrored_entries_without_duplicates() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        let restored = prompt("one", None);
        let restored_id = restored.queued_id.unwrap();

        // A restart emptied the memory copy; the document's mirrors refill it.
        state.seed_queue(session, vec![restored, prompt("two", None)]);

        // Seeding is additive only for ids the queue does not already hold.
        state.seed_queue(
            session,
            vec![
                AgentPrompt {
                    prompt: "one rewritten".into(),
                    transport: None,
                    sender: None,
                    queued_id: Some(restored_id),
                    context: None,
                },
                prompt("three", None),
            ],
        );
        let order: Vec<String> = [
            state.pop_queued(session),
            state.pop_queued(session),
            state.pop_queued(session),
        ]
        .into_iter()
        .flatten()
        .map(|entry| entry.prompt)
        .collect();
        assert_eq!(order, ["one", "two", "three"]);
    }

    #[test]
    fn a_rejected_queued_steer_requeues_but_a_direct_one_drops() {
        let state = AgentState::default();
        let session = Uuid::new_v4();

        let queued = prompt("drained", None);
        let queued_id = queued.queued_id.unwrap();
        state.record_pending_steer(session, queued);
        state.record_pending_steer(
            session,
            AgentPrompt {
                prompt: "direct".into(),
                transport: None,
                sender: None,
                queued_id: None,
                context: None,
            },
        );

        let _ = state.note_driver_event(
            session,
            &DriverEvent::SteerRejected {
                message: "drained".into(),
                reason: "turn ended".into(),
                reason_i18n: None,
                hidden: false,
            },
        );
        // The mirrored chip's prompt returns to the head of the queue.
        assert_eq!(
            state.pop_queued(session).unwrap().queued_id,
            Some(queued_id)
        );

        let _ = state.note_driver_event(
            session,
            &DriverEvent::SteerRejected {
                message: "direct".into(),
                reason: "turn ended".into(),
                reason_i18n: None,
                hidden: false,
            },
        );
        assert!(state.pop_queued(session).is_none());
    }

    #[test]
    fn a_rejected_context_steer_returns_to_the_caller_unrequeued() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        state.record_pending_steer(
            session,
            AgentPrompt {
                prompt: "context".into(),
                transport: None,
                sender: None,
                queued_id: None,
                context: Some(ContextSteer::Memory),
            },
        );
        assert!(state.context_steer_pending(session));

        let rejected = state
            .note_driver_event(
                session,
                &DriverEvent::SteerRejected {
                    message: "context".into(),
                    reason: "turn ended".into(),
                    reason_i18n: None,
                    hidden: false,
                },
            )
            .expect("the rejected context steer resolves to its record");
        assert_eq!(rejected.context, Some(ContextSteer::Memory));
        // Not requeued, and the session is free to retry the injection.
        assert!(!state.context_steer_pending(session));
        assert!(state.pop_queued(session).is_none());
    }

    #[test]
    fn turn_events_decide_where_a_prompt_waits() {
        let state = AgentState::default();
        let session = Uuid::new_v4();

        assert!(!state.has_open_turn(session));
        assert!(!state.is_working(session));
        assert!(!state.has_parked_turn(session));

        let _ = state.note_driver_event(session, &DriverEvent::TurnStarted);
        assert!(state.has_open_turn(session));
        assert!(state.is_working(session));
        assert!(!state.has_parked_turn(session));

        let _ = state.note_driver_event(session, &DriverEvent::TurnParked);
        assert!(state.has_open_turn(session));
        assert!(!state.is_working(session));
        assert!(state.has_parked_turn(session));

        let _ = state.note_driver_event(
            session,
            &DriverEvent::TurnFinished {
                success: true,
                summary: None,
                summary_i18n: None,
            },
        );
        assert!(!state.has_open_turn(session));
        assert!(!state.is_working(session));
    }

    #[test]
    fn an_accepted_steer_releases_its_sender_provenance() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        let sender = Uuid::new_v4();
        state.record_pending_steer(session, prompt("first", Some(sender)));
        state.record_pending_steer(session, prompt("second", None));

        // An exact echo takes its own entry, not the oldest.
        let taken = state.take_pending_steer(session, "second").unwrap();
        assert_eq!(taken.prompt, "second");
        assert_eq!(taken.sender, None);

        // A normalized echo releases the oldest pending steer.
        let taken = state
            .take_pending_steer(session, "unrecognized echo")
            .unwrap();
        assert_eq!(taken.prompt, "first");
        assert_eq!(taken.sender, Some(sender));
        assert!(state.take_pending_steer(session, "anything").is_none());
    }

    #[test]
    fn an_enveloped_steer_resolves_on_the_transport_echo() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        let sender = Uuid::new_v4();
        let mut wrapped = prompt("the sender's own words", Some(sender));
        wrapped.transport = Some("envelope\n\nthe sender's own words".into());
        state.record_pending_steer(session, wrapped);

        // The provider echoes the transport text; the popped record still
        // carries the sender's own words for the transcript.
        let taken = state
            .take_pending_steer(session, "envelope\n\nthe sender's own words")
            .unwrap();
        assert_eq!(taken.prompt, "the sender's own words");
        assert_eq!(taken.sender, Some(sender));
    }

    #[test]
    fn process_exit_drops_pending_steers_and_turn_state() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        let _ = state.note_driver_event(session, &DriverEvent::TurnStarted);
        state.record_pending_steer(session, prompt("in flight", Some(Uuid::new_v4())));

        let _ = state.note_driver_event(session, &DriverEvent::ProcessExited);

        assert!(!state.has_open_turn(session));
        assert!(state.take_pending_steer(session, "in flight").is_none());
    }

    fn launch_env(directory: &Path) -> AgentLaunchEnv {
        AgentLaunchEnv {
            token: "scoped-token".to_owned(),
            task_id: Uuid::new_v4(),
            parent_task_id: None,
            daemon_address: "127.0.0.1:7777".to_owned(),
            cli_path: PathBuf::from("/waku/bin/goddard-agent"),
            shim_directory: directory.to_path_buf(),
            task_tools: true,
            settings_writes: true,
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_session_shim_execs_the_cli_with_the_scoped_credential() {
        let directory = std::env::temp_dir().join(format!("goddard-agent-test-{}", Uuid::new_v4()));
        let env = launch_env(&directory);

        let shim = write_session_shim(&env).expect("the shim should be written");

        let script = std::fs::read_to_string(&shim).expect("the shim is readable");
        assert!(script.starts_with("#!/bin/sh"));
        assert!(script.contains("exec env"));
        assert!(script.contains("GODDARD_AGENT_TOKEN='scoped-token'"));
        assert!(script.contains(&format!("GODDARD_TASK_ID='{}'", env.task_id)));
        assert!(script.contains("GODDARD_DAEMON_ADDRESS='127.0.0.1:7777'"));
        assert!(script.contains("'/waku/bin/goddard-agent' \"$@\""));

        use std::os::unix::fs::PermissionsExt as _;
        let mode = shim.metadata().unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "the shim is private and executable");

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[cfg(unix)]
    #[test]
    fn a_side_chat_shim_carries_its_parent_task_id() {
        let directory = std::env::temp_dir().join(format!("goddard-agent-test-{}", Uuid::new_v4()));
        let parent = Uuid::new_v4();
        let env = AgentLaunchEnv {
            parent_task_id: Some(parent),
            ..launch_env(&directory)
        };

        let shim = write_session_shim(&env).expect("the shim should be written");
        let script = std::fs::read_to_string(&shim).expect("the shim is readable");
        assert!(script.contains(&format!("GODDARD_PARENT_TASK_ID='{parent}'")));

        // A plain session's shim carries no parent reference at all.
        let shim =
            write_session_shim(&launch_env(&directory)).expect("the plain shim should be written");
        let script = std::fs::read_to_string(&shim).expect("the plain shim is readable");
        assert!(!script.contains("GODDARD_PARENT_TASK_ID"));

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_shared_service_instruction_names_the_shim_and_the_contract() {
        let directory = std::env::temp_dir().join(format!("goddard-agent-test-{}", Uuid::new_v4()));
        let env = launch_env(&directory);
        let instruction = shared_service_instruction(Path::new("/x/goddard-agent"), &env);
        assert!(instruction.starts_with("<goddard-agent>\n"));
        assert!(instruction.ends_with("\n</goddard-agent>"));
        assert!(instruction.contains("/x/goddard-agent"));
        assert!(instruction.contains("only when the user asks"));
        assert!(instruction.contains("create, start, or spawn"));
        assert!(instruction.contains("`command`"));

        // Each scope drops its own half of the contract when disabled.
        let env = AgentLaunchEnv {
            task_tools: false,
            ..launch_env(&directory)
        };
        let instruction = shared_service_instruction(Path::new("/x/goddard-agent"), &env);
        assert!(instruction.contains("`command`"));
        assert!(!instruction.contains("only when the user asks"));
        // The scoped read is still in the surface: every credential reads
        // its own task's transcript.
        assert!(instruction.contains("read this task's transcript"));
        assert!(instruction.contains("\"turn\""));

        // A side chat without the cross-task surface still names its parent.
        let parent = Uuid::new_v4();
        let env = AgentLaunchEnv {
            task_tools: false,
            settings_writes: false,
            parent_task_id: Some(parent),
            ..launch_env(&directory)
        };
        let instruction = shared_service_instruction(Path::new("/x/goddard-agent"), &env);
        assert!(instruction.contains(&format!("side chat of task {parent}")));

        let env = AgentLaunchEnv {
            task_tools: true,
            settings_writes: false,
            ..launch_env(&directory)
        };
        let instruction = shared_service_instruction(Path::new("/x/goddard-agent"), &env);
        assert!(!instruction.contains("`command`"));
        assert!(instruction.contains("only when the user asks"));
    }

    #[test]
    fn the_surface_block_is_owed_once_until_announced_or_revoked() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        let scope = AgentSurfaceScope {
            task_tools: true,
            settings_writes: false,
            parent_task_id: None,
        };
        assert!(state.surface_block(session).is_none());

        state.note_surface(session, scope);
        let block = state.surface_block(session).expect("the surface is owed");
        assert!(block.contains("`goddard-agent`"));
        // Composing the block does not announce it — the steer path waits
        // for the provider's accept echo.
        assert!(state.surface_block(session).is_some());

        state.mark_surface_announced(session);
        assert!(state.surface_block(session).is_none());

        // A fresh runtime is owed the announcement again.
        state.note_surface(session, scope);
        assert!(state.surface_block(session).is_some());
        state.revoke_session(session);
        assert!(state.surface_block(session).is_none());
    }

    #[test]
    fn clear_session_forgets_credentials_queue_and_turns() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        let token = state.mint(session);
        state.enqueue(session, prompt("held", None));
        let _ = state.note_driver_event(session, &DriverEvent::TurnStarted);

        state.clear_session(session);

        assert_eq!(state.resolve(&token), None);
        assert!(state.pop_queued(session).is_none());
        assert!(!state.has_open_turn(session));
    }
}
