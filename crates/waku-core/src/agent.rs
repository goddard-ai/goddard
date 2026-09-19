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

use std::collections::{HashMap, VecDeque};
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
    pub prompt: String,
    /// The task whose agent sent it. `None` is possible only for requests
    /// made with the master token, which carries no session scope.
    pub sender: Option<Uuid>,
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

/// Shared agent surface state. One instance lives on the backend; the runtime
/// event forwarder holds a second reference so it can track turns, drain
/// queues, and attach provenance to echoed steers without round-tripping
/// through the request path.
#[derive(Default)]
pub struct AgentState {
    /// Scoped bearer token → the session owning the runtime it was minted
    /// for. Several tokens can name the same session when a task restarts.
    tokens: Mutex<HashMap<String, Uuid>>,
    /// Queue-mode prompts waiting for the target session to become idle.
    queues: Mutex<HashMap<Uuid, VecDeque<AgentPrompt>>>,
    /// Steer injections in flight, oldest first.
    pending_steers: Mutex<HashMap<Uuid, VecDeque<AgentPrompt>>>,
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
    /// only while the provider process that carries it lives.
    pub fn revoke_session(&self, session_id: Uuid) {
        self.tokens.lock().retain(|_, owner| *owner != session_id);
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
        self.queues.lock().clear();
        self.pending_steers.lock().clear();
        self.turns.lock().clear();
    }

    /// Update the session's turn bookkeeping from a runtime event.
    pub fn note_driver_event(&self, session_id: Uuid, event: &DriverEvent) {
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
            }
            DriverEvent::SteerRejected { message, .. } => {
                self.take_pending_steer(session_id, message);
            }
            _ => {}
        }
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
            .position(|steer| steer.prompt == message)
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
        let script = format!(
            "#!/bin/sh\nexec env {}='{}' {}='{}' {}='{}' '{}' \"$@\"\n",
            waku_protocol::DAEMON_ADDRESS_ENV,
            shell_quote_escape(&env.daemon_address),
            waku_protocol::AGENT_TOKEN_ENV,
            shell_quote_escape(&env.token),
            waku_protocol::AGENT_TASK_ENV,
            env.task_id,
            shell_quote_escape(&env.cli_path.display().to_string()),
        );
        write_private_executable(&path, script.as_bytes())?;
        path
    };
    #[cfg(windows)]
    let shim = {
        let path = env.shim_directory.join("goddard-agent.cmd");
        let script = format!(
            "@echo off\r\nset \"{}={}\"\r\nset \"{}={}\"\r\nset \"{}={}\"\r\n\"{}\" %*\r\n",
            waku_protocol::DAEMON_ADDRESS_ENV,
            env.daemon_address,
            waku_protocol::AGENT_TOKEN_ENV,
            env.token,
            waku_protocol::AGENT_TASK_ENV,
            env.task_id,
            env.cli_path.display(),
        );
        write_private_executable(&path, script.as_bytes())?;
        path
    };
    Ok(shim)
}

/// The prompt shared-service sessions get in place of a launch environment:
/// the same contract the CLI's own help states, plus the session-scoped
/// launcher's path and which credential scopes this session carries.
pub fn shared_service_instruction(shim: &Path, env: &AgentLaunchEnv) -> String {
    let mut instruction = format!(
        "Goddard exposes a scoped agent surface to this session through `{shim}`; `{shim} --help` documents every subcommand and its JSON payload.",
        shim = shim.display()
    );
    if env.settings_writes {
        instruction.push_str(
            " The `command` subcommands manage the user's custom commands — use them whenever adding one would help the human, not only when asked.",
        );
    }
    if env.task_tools {
        instruction.push_str(
            " When — and only when — the human explicitly asks you to create another task or send a message to one, use `create` and `prompt`; those calls are attributed to this task in the target's transcript. Do not use them for exploration, convenience, or self-orchestration.",
        );
    }
    instruction
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
            sender,
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
    fn turn_events_decide_where_a_prompt_waits() {
        let state = AgentState::default();
        let session = Uuid::new_v4();

        assert!(!state.has_open_turn(session));
        assert!(!state.is_working(session));
        assert!(!state.has_parked_turn(session));

        state.note_driver_event(session, &DriverEvent::TurnStarted);
        assert!(state.has_open_turn(session));
        assert!(state.is_working(session));
        assert!(!state.has_parked_turn(session));

        state.note_driver_event(session, &DriverEvent::TurnParked);
        assert!(state.has_open_turn(session));
        assert!(!state.is_working(session));
        assert!(state.has_parked_turn(session));

        state.note_driver_event(
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
    fn process_exit_drops_pending_steers_and_turn_state() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        state.note_driver_event(session, &DriverEvent::TurnStarted);
        state.record_pending_steer(session, prompt("in flight", Some(Uuid::new_v4())));

        state.note_driver_event(session, &DriverEvent::ProcessExited);

        assert!(!state.has_open_turn(session));
        assert!(state.take_pending_steer(session, "in flight").is_none());
    }

    fn launch_env(directory: &Path) -> AgentLaunchEnv {
        AgentLaunchEnv {
            token: "scoped-token".to_owned(),
            task_id: Uuid::new_v4(),
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

    #[test]
    fn the_shared_service_instruction_names_the_shim_and_the_contract() {
        let directory = std::env::temp_dir().join(format!("goddard-agent-test-{}", Uuid::new_v4()));
        let env = launch_env(&directory);
        let instruction = shared_service_instruction(Path::new("/x/goddard-agent"), &env);
        assert!(instruction.contains("/x/goddard-agent"));
        assert!(instruction.contains("explicitly asks"));
        assert!(instruction.contains("`command`"));

        // Each scope drops its own half of the contract when disabled.
        let env = AgentLaunchEnv {
            task_tools: false,
            ..launch_env(&directory)
        };
        let instruction = shared_service_instruction(Path::new("/x/goddard-agent"), &env);
        assert!(instruction.contains("`command`"));
        assert!(!instruction.contains("explicitly asks"));

        let env = AgentLaunchEnv {
            task_tools: true,
            settings_writes: false,
            ..launch_env(&directory)
        };
        let instruction = shared_service_instruction(Path::new("/x/goddard-agent"), &env);
        assert!(!instruction.contains("`command`"));
        assert!(instruction.contains("explicitly asks"));
    }

    #[test]
    fn clear_session_forgets_credentials_queue_and_turns() {
        let state = AgentState::default();
        let session = Uuid::new_v4();
        let token = state.mint(session);
        state.enqueue(session, prompt("held", None));
        state.note_driver_event(session, &DriverEvent::TurnStarted);

        state.clear_session(session);

        assert_eq!(state.resolve(&token), None);
        assert!(state.pop_queued(session).is_none());
        assert!(!state.has_open_turn(session));
    }
}
