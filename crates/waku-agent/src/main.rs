//! `goddard-agent`: the scoped control surface Goddard exposes to agents running
//! inside a provider session.
//!
//! The daemon places this binary on the session's `PATH` together with a
//! per-session credential (`GODDARD_AGENT_TOKEN`), this session's task id
//! (`GODDARD_TASK_ID`), and the daemon address (`GODDARD_DAEMON_ADDRESS`). The
//! token grants only the commands below — nothing else — and dies with the
//! session.
//!
//! Usage contract: `command` manages the user's settings — today their
//! custom commands — and is available whenever changing one would help them.
//! Invoke `create` or `prompt` only when the human you are working for has
//! explicitly asked you to create another task or to send a message to one.
//! There is no per-call approval gate; the daemon stamps every accepted
//! write with this task's id so agent-originated changes stay visible to the
//! user.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context as _, anyhow, bail};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use waku_client::DaemonClient;
use waku_protocol::custom_commands::{CustomCommand, CustomCommandIcon};
use waku_protocol::model::ProviderKind;
use waku_protocol::{
    AGENT_TASK_ENV, AGENT_TOKEN_ENV, AgentPromptDelivery, AgentWorkspace, Command,
    DAEMON_ADDRESS_ENV, ResponsePayload,
};

const USAGE: &str = "\
goddard-agent — Goddard's scoped agent surface inside a session

USAGE
    goddard-agent create '<json>'            Create a task and start its first prompt
    goddard-agent prompt '<json>'            Send a prompt to an existing task
    goddard-agent read '<json>'              Read a task's transcript
    goddard-agent command list               List the user's custom commands
    goddard-agent command upsert '<json>'    Add or update a custom command
    goddard-agent command remove '<json>'    Remove a custom command
    goddard-agent schema                     Print the JSON payload schemas
    goddard-agent --help                     Show this text

USAGE CONTRACT
    `command` manages the user's settings — today their custom commands —
    and is available whenever changing a setting would help them.
    `create`, `prompt`, and foreign `read` are the cross-task surface. When
    the human asks you to create, start, or spawn another task or session —
    including running work in a separate task — use `create`; when they ask
    you to send a message to another task, use `prompt`. `read` is the read
    half of that surface — use it when another task's transcript holds
    context you need, for example when GODDARD_PARENT_TASK_ID names the
    task this session is a side chat of; with no address fields it reads
    this task's own transcript, which is how context handed off across a
    provider switch stays reachable. Use `create` and `prompt` only when
    the human you are working for has explicitly asked — never for
    exploration,
    convenience, or self-orchestration.
    There is no per-call approval gate for either surface; the daemon records
    this task's id on every accepted write, so agent-originated commands and
    turns are visibly attributed to it.

ENVIRONMENT
    GODDARD_DAEMON_ADDRESS   Daemon WebSocket address (injected by the daemon)
    GODDARD_AGENT_TOKEN      Per-session scoped credential (injected)
    GODDARD_TASK_ID          This session's task id (injected)
    GODDARD_PARENT_TASK_ID   Parent task id — side-chat sessions only (injected)

Run `goddard-agent schema` for the accepted payloads.";

fn schema() -> serde_json::Value {
    let icons: Vec<String> = CustomCommandIcon::ALL
        .iter()
        .filter_map(|icon| serde_json::to_value(icon).ok()?.as_str().map(str::to_owned))
        .collect();
    json!({
        "usage_contract": "`command` manages the user's settings — today their custom commands — and is available whenever changing a setting would help them. `create` and `prompt` are the cross-task surface: only invoke them when the human you are working for has explicitly asked you to create another task or to send a message to one. There is no per-call approval gate; the daemon records this task's id on every accepted write so agent-originated changes stay visibly attributed.",
        "create": {
            "description": "Create a fully configured task and immediately start its first prompt. There is no idle-task creation.",
            "fields": {
                "provider": {"type": "string", "enum": ["amp", "claude", "codex", "cursor", "deepseek", "devin", "fx", "opencode", "opencode2", "goose", "grok", "kimi", "muse", "ohmypi", "pi"], "notes": "omit to run the new task on this task's provider"},
                "model": {"type": "string", "notes": "explicit provider model id; \"default\" selects the provider's own default; omit to inherit this task's model when it runs the resolved provider"},
                "project": {"type": "string", "required": true, "notes": "absolute path; resolves an existing project or registers a primary Git checkout (linked worktrees are rejected)"},
                "workspace": {"type": "string", "required": true, "enum": ["local", "worktree"]},
                "base_branch": {"type": "string", "required_when": "workspace == \"worktree\"", "notes": "ignored for \"local\""},
                "prompt": {"type": "string", "required": true},
                "reasoning_effort": {"type": "string", "notes": "provider-specific effort id; \"default\" selects the provider's own default; omit to inherit this task's effort when it runs the resolved provider (falls back to the model's default when the resolved model does not list it)"},
                "service_tier": {"type": "string", "notes": "provider-specific tier id; inherits like reasoning_effort"},
                "context_window": {"type": "string", "notes": "provider-specific window id; inherits like reasoning_effort"}
            },
            "example": "{\"project\":\"/abs/path\",\"workspace\":\"worktree\",\"base_branch\":\"main\",\"prompt\":\"Summarize the diff\"}",
            "returns": {"task_id": "uuid of the created task"}
        },
        "prompt": {
            "description": "Submit a prompt to an existing task, addressed by Goddard task id or provider-native thread id.",
            "fields": {
                "task_id": {"type": "string", "notes": "Goddard task UUID; exactly one of task_id and thread_id is required"},
                "thread_id": {"type": "string", "notes": "provider-native Agent CLI thread id; exactly one of task_id and thread_id is required"},
                "provider": {"type": "string", "notes": "disambiguates thread_id when several tasks share it"},
                "prompt": {"type": "string", "required": true},
                "delivery": {"type": "string", "enum": ["queue", "steer"], "default": "queue", "notes": "queue waits for the target to go idle and preserves submission order; steer injects into the running turn and fails when no turn is running"}
            },
            "example": "{\"task_id\":\"<uuid>\",\"prompt\":\"How is the migration going?\",\"delivery\":\"queue\"}",
            "returns": {"ok": true}
        },
        "read": {
            "description": "Read a task's transcript: its title, provider, status, and transcript entries — messages and tool activity — in order, each tagged with its 1-based turn number. With no address fields it reads this task's own transcript; a side chat's parent task id is in GODDARD_PARENT_TASK_ID. Entries carry a `turn` number so `turn` can re-read one turn in full.",
            "fields": {
                "task_id": {"type": "string", "notes": "Goddard task UUID; omit with thread_id to read this task's own transcript"},
                "thread_id": {"type": "string", "notes": "provider-native Agent CLI thread id; exactly one of task_id and thread_id is required for a foreign read"},
                "provider": {"type": "string", "notes": "disambiguates thread_id when several tasks share it"},
                "turn": {"type": "number", "notes": "1-based turn number; restricts the answer to that turn's entries"}
            },
            "example": "{\"turn\":3}",
            "returns": {"task_id": "uuid", "title": "string", "provider": "string", "status": "string", "items": [{"turn": "1-based turn number when the entry belongs to one", "kind": "message|activity", "role": "user|assistant|system on message items", "content": "string"}], "truncated": "true when the size cap dropped the oldest items"}
        },
        "command": {
            "description": "Manage the user's custom commands — shell scripts they can run from the command palette in a terminal. Commands are daemon-owned and shared across the user's clients.",
            "subcommands": {
                "list": {
                    "description": "Print every custom command as a JSON array. Read this first to make writes idempotent.",
                    "returns": "the custom command list"
                },
                "upsert": {
                    "description": "Add a custom command, or replace the entry carrying `id` — or the one with the same `name` when `id` is absent or unknown. The daemon attributes the write to this task.",
                    "fields": {
                        "id": {"type": "string", "notes": "uuid of an existing command; omit to add a new one"},
                        "name": {"type": "string", "notes": "palette label; omit to show the script itself"},
                        "icon": {"type": "string", "enum": icons, "default": "terminal"},
                        "shell": {"type": "string", "notes": "shell the command runs in; omit for the platform default"},
                        "script": {"type": "string", "required": true, "notes": "runs inside an interactive shell, so pipes, aliases, and interactive programs all work"},
                        "close_on_success": {"type": "boolean", "default": false, "notes": "close the terminal tab once the script exits successfully"}
                    },
                    "example": "{\"name\":\"Deploy staging\",\"script\":\"./scripts/deploy staging\",\"icon\":\"zap\",\"close_on_success\":true}",
                    "returns": "the custom command list after the write"
                },
                "remove": {
                    "description": "Remove a custom command, addressed by id or by its exact name.",
                    "fields": {
                        "id": {"type": "string", "notes": "uuid of the command; one of id and name is required"},
                        "name": {"type": "string", "notes": "exact name of the command; one of id and name is required"}
                    },
                    "example": "{\"name\":\"Deploy staging\"}",
                    "returns": "the custom command list after the write"
                }
            }
        }
    })
}

#[derive(Deserialize)]
struct CreatePayload {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    project: PathBuf,
    workspace: AgentWorkspaceArg,
    #[serde(default)]
    base_branch: Option<String>,
    prompt: String,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    service_tier: Option<String>,
    #[serde(default)]
    context_window: Option<String>,
}

#[derive(Deserialize)]
struct PromptPayload {
    #[serde(default)]
    task_id: Option<Uuid>,
    #[serde(default)]
    thread_id: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    prompt: String,
    #[serde(default)]
    delivery: DeliveryArg,
}

// `read` — a transcript, not the task. With neither address field the
// daemon reads the caller's own task.
#[derive(Deserialize)]
struct ReadPayload {
    #[serde(default)]
    task_id: Option<Uuid>,
    #[serde(default)]
    thread_id: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    /// Restrict the answer to one turn's entries, by its 1-based turn
    /// number.
    #[serde(default)]
    turn: Option<usize>,
}

#[derive(Deserialize)]
struct CommandUpsertPayload {
    #[serde(default)]
    id: Option<Uuid>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    icon: Option<CustomCommandIcon>,
    #[serde(default)]
    shell: Option<String>,
    script: String,
    #[serde(default)]
    close_on_success: bool,
}

#[derive(Deserialize)]
struct CommandRemovePayload {
    #[serde(default)]
    id: Option<Uuid>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AgentWorkspaceArg {
    Local,
    Worktree,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DeliveryArg {
    #[default]
    Queue,
    Steer,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("goddard-agent: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let mut arguments = std::env::args().skip(1);
    let subcommand = arguments.next().unwrap_or_default();
    match subcommand.as_str() {
        "--help" | "-h" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        "schema" => {
            println!("{}", serde_json::to_string_pretty(&schema())?);
            Ok(())
        }
        "command" => command(arguments.next().as_deref(), arguments.next()),
        "create" | "prompt" | "read" => {
            let payload = arguments
                .next()
                .ok_or_else(|| anyhow!("`{subcommand}` takes one JSON object argument; run `goddard-agent schema` for its shape"))?;
            if arguments.next().is_some() {
                bail!("`{subcommand}` accepts exactly one JSON object argument");
            }
            let command = build_command(&subcommand, &payload)?;
            let response = connect()?.request(request_session_id(), Uuid::nil(), command)?;
            match response {
                ResponsePayload::AgentSessionCreated { session_id } => {
                    println!("{}", serde_json::json!({ "task_id": session_id }));
                }
                ResponsePayload::AgentSessionTranscript { transcript } => {
                    println!("{}", serde_json::to_string_pretty(&transcript)?);
                }
                ResponsePayload::Ack => {
                    println!("{}", serde_json::json!({ "ok": true }));
                }
                other => bail!("daemon returned an unexpected response: {other:?}"),
            }
            Ok(())
        }
        "" => {
            eprintln!("{USAGE}");
            Err(anyhow!("a subcommand is required"))
        }
        other => Err(anyhow!(
            "unknown subcommand `{other}`; run `goddard-agent --help`"
        )),
    }
}

fn command(action: Option<&str>, payload: Option<String>) -> anyhow::Result<()> {
    let request = match action {
        Some("list") => Command::ListCustomCommands,
        Some("upsert") => Command::UpsertCustomCommand {
            command: upsert_payload(&payload)?,
        },
        Some("remove") => {
            let payload: CommandRemovePayload = serde_json::from_str(
                payload
                    .as_deref()
                    .ok_or_else(|| anyhow!("`command remove` takes one JSON object argument"))?,
            )
            .context(
                "`command remove` takes a JSON object; run `goddard-agent schema` for its shape",
            )?;
            Command::RemoveCustomCommand {
                id: payload.id,
                name: payload.name,
            }
        }
        _ => bail!("`command` takes one of `list`, `upsert`, or `remove`"),
    };
    match connect()?.request(request_session_id(), Uuid::nil(), request)? {
        ResponsePayload::CustomCommands { commands } => {
            println!("{}", serde_json::to_string_pretty(&commands)?);
            Ok(())
        }
        other => bail!("daemon returned an unexpected response: {other:?}"),
    }
}

fn upsert_payload(payload: &Option<String>) -> anyhow::Result<CustomCommand> {
    let payload: CommandUpsertPayload = serde_json::from_str(
        payload
            .as_deref()
            .ok_or_else(|| anyhow!("`command upsert` takes one JSON object argument"))?,
    )
    .context("`command upsert` takes a JSON object; run `goddard-agent schema` for its shape")?;
    if payload.script.trim().is_empty() {
        bail!("`command upsert` requires a non-empty `script`");
    }
    Ok(CustomCommand {
        id: payload.id.unwrap_or_else(Uuid::nil),
        name: payload.name,
        icon: payload.icon.unwrap_or_default(),
        shell: payload.shell,
        script: payload.script,
        close_on_success: payload.close_on_success,
        created_by_task: None,
    })
}

fn build_command(subcommand: &str, payload: &str) -> anyhow::Result<Command> {
    match subcommand {
        "create" => {
            let payload: CreatePayload = serde_json::from_str(payload).context(
                "`create` takes a JSON object; run `goddard-agent schema` for its shape",
            )?;
            Ok(Command::AgentCreateSession {
                provider: payload.provider.as_deref().map(provider_kind).transpose()?,
                model: payload.model,
                project: payload.project,
                workspace: match payload.workspace {
                    AgentWorkspaceArg::Local => AgentWorkspace::Local,
                    AgentWorkspaceArg::Worktree => AgentWorkspace::Worktree,
                },
                base_branch: payload.base_branch,
                prompt: payload.prompt,
                reasoning_effort: payload.reasoning_effort,
                service_tier: payload.service_tier,
                context_window: payload.context_window,
            })
        }
        "prompt" => {
            let payload: PromptPayload = serde_json::from_str(payload).context(
                "`prompt` takes a JSON object; run `goddard-agent schema` for its shape",
            )?;
            let provider = payload.provider.as_deref().map(provider_kind).transpose()?;
            Ok(Command::AgentPrompt {
                task_id: payload.task_id,
                thread_id: payload.thread_id,
                provider,
                prompt: payload.prompt,
                delivery: match payload.delivery {
                    DeliveryArg::Queue => AgentPromptDelivery::Queue,
                    DeliveryArg::Steer => AgentPromptDelivery::Steer,
                },
            })
        }
        "read" => {
            let payload: ReadPayload = serde_json::from_str(payload)
                .context("`read` takes a JSON object; run `goddard-agent schema` for its shape")?;
            let provider = payload.provider.as_deref().map(provider_kind).transpose()?;
            Ok(Command::AgentReadSession {
                task_id: payload.task_id,
                thread_id: payload.thread_id,
                provider,
                turn: payload.turn,
            })
        }
        _ => unreachable!("checked by run()"),
    }
}

fn provider_kind(id: &str) -> anyhow::Result<ProviderKind> {
    let normalized = id.trim().to_ascii_lowercase();
    ProviderKind::ALL
        .iter()
        .copied()
        .find(|kind| kind.id() == normalized)
        .ok_or_else(|| {
            let known = ProviderKind::ALL
                .iter()
                .copied()
                .map(ProviderKind::id)
                .collect::<Vec<_>>()
                .join(", ");
            anyhow!("unknown provider `{id}`; expected one of: {known}")
        })
}

fn connect() -> anyhow::Result<DaemonClient> {
    let address = std::env::var(DAEMON_ADDRESS_ENV)
        .context("GODDARD_DAEMON_ADDRESS is not set; this session has no agent surface")?;
    let token = std::env::var(AGENT_TOKEN_ENV)
        .context("GODDARD_AGENT_TOKEN is not set; this session has no agent surface")?;
    DaemonClient::connect(&address, token).context("could not reach the Goddard daemon")
}

fn request_session_id() -> Uuid {
    std::env::var(AGENT_TASK_ENV)
        .ok()
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(Uuid::nil)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_create_payload_becomes_an_agent_create_command() {
        let command = build_command(
            "create",
            r#"{"provider":"codex","model":"default","project":"/tmp/project","workspace":"worktree","base_branch":"main","prompt":"Summarize the diff","reasoning_effort":"high"}"#,
        )
        .expect("a valid create payload parses");

        match command {
            Command::AgentCreateSession {
                provider,
                model,
                project,
                workspace,
                base_branch,
                prompt,
                reasoning_effort,
                service_tier,
                context_window,
            } => {
                assert_eq!(provider, Some(ProviderKind::Codex));
                assert_eq!(model.as_deref(), Some("default"));
                assert_eq!(project, PathBuf::from("/tmp/project"));
                assert_eq!(workspace, AgentWorkspace::Worktree);
                assert_eq!(base_branch.as_deref(), Some("main"));
                assert_eq!(prompt, "Summarize the diff");
                assert_eq!(reasoning_effort.as_deref(), Some("high"));
                assert_eq!(service_tier, None);
                assert_eq!(context_window, None);
            }
            other => panic!("expected AgentCreateSession, got {other:?}"),
        }
    }

    #[test]
    fn a_create_payload_may_omit_provider_model_and_traits_to_inherit() {
        let command = build_command(
            "create",
            r#"{"project":"/tmp/project","workspace":"local","prompt":"Summarize the diff"}"#,
        )
        .expect("a create payload without provider or traits parses");

        match command {
            Command::AgentCreateSession {
                provider,
                model,
                reasoning_effort,
                service_tier,
                context_window,
                ..
            } => {
                assert_eq!(provider, None);
                assert_eq!(model, None);
                assert_eq!(reasoning_effort, None);
                assert_eq!(service_tier, None);
                assert_eq!(context_window, None);
            }
            other => panic!("expected AgentCreateSession, got {other:?}"),
        }
    }

    #[test]
    fn a_prompt_payload_defaults_to_queue_delivery() {
        let task_id = Uuid::new_v4();
        let payload = format!(r#"{{"task_id":"{task_id}","prompt":"status?"}}"#);
        let command = build_command("prompt", &payload).expect("a task-id prompt parses");

        match command {
            Command::AgentPrompt {
                task_id: target,
                thread_id,
                prompt,
                delivery,
                ..
            } => {
                assert_eq!(target, Some(task_id));
                assert_eq!(thread_id, None);
                assert_eq!(prompt, "status?");
                assert_eq!(delivery, AgentPromptDelivery::Queue);
            }
            other => panic!("expected AgentPrompt, got {other:?}"),
        }
    }

    #[test]
    fn a_read_payload_becomes_an_agent_read_command() {
        let task_id = Uuid::new_v4();
        let payload = format!(r#"{{"task_id":"{task_id}","turn":3}}"#);
        let command = build_command("read", &payload).expect("a task-id read parses");

        match command {
            Command::AgentReadSession {
                task_id: target,
                thread_id,
                provider,
                turn,
            } => {
                assert_eq!(target, Some(task_id));
                assert_eq!(thread_id, None);
                assert_eq!(provider, None);
                assert_eq!(turn, Some(3));
            }
            other => panic!("expected AgentReadSession, got {other:?}"),
        }

        // An empty payload reads the caller's own task — the daemon
        // resolves the scoped credential.
        let command = build_command("read", "{}").expect("a bare read parses");
        match command {
            Command::AgentReadSession {
                task_id,
                thread_id,
                provider,
                turn,
            } => {
                assert_eq!(
                    (task_id, thread_id, provider, turn),
                    (None, None, None, None)
                );
            }
            other => panic!("expected AgentReadSession, got {other:?}"),
        }
    }

    #[test]
    fn a_prompt_payload_accepts_a_thread_id_with_a_provider_and_steer() {
        let command = build_command(
            "prompt",
            r#"{"thread_id":"thread-9","provider":"claude","prompt":"keep going","delivery":"steer"}"#,
        )
        .expect("a thread-id prompt parses");

        match command {
            Command::AgentPrompt {
                task_id,
                thread_id,
                provider,
                delivery,
                ..
            } => {
                assert_eq!(task_id, None);
                assert_eq!(thread_id.as_deref(), Some("thread-9"));
                assert_eq!(provider, Some(ProviderKind::Claude));
                assert_eq!(delivery, AgentPromptDelivery::Steer);
            }
            other => panic!("expected AgentPrompt, got {other:?}"),
        }
    }

    #[test]
    fn malformed_json_and_unknown_providers_are_errors() {
        assert!(build_command("create", "not json").is_err());
        assert!(
            build_command(
                "create",
                r#"{"provider":"hal","model":"default","project":"/tmp","workspace":"local","prompt":"x"}"#,
            )
            .is_err()
        );
        assert!(build_command("prompt", r#"{"delivery":"sideways","prompt":"x"}"#).is_err());
    }

    #[test]
    fn an_upsert_payload_becomes_a_custom_command() {
        let id = Uuid::new_v4();
        let payload = format!(
            r#"{{"id":"{id}","name":"Deploy staging","icon":"zap","script":"./deploy","close_on_success":true}}"#
        );
        let command = upsert_payload(&Some(payload)).expect("a valid upsert parses");
        assert_eq!(command.id, id);
        assert_eq!(command.name.as_deref(), Some("Deploy staging"));
        assert_eq!(command.icon, CustomCommandIcon::Zap);
        assert_eq!(command.script, "./deploy");
        assert!(command.close_on_success);
        assert_eq!(command.created_by_task, None);
    }

    #[test]
    fn an_upsert_without_an_id_or_defaults_still_parses() {
        let command = upsert_payload(&Some(r#"{"script":"echo hi"}"#.to_owned()))
            .expect("only the script is required");
        assert!(command.id.is_nil());
        assert_eq!(command.name, None);
        assert_eq!(command.icon, CustomCommandIcon::Terminal);
        assert!(!command.close_on_success);
    }

    #[test]
    fn an_upsert_rejects_blank_scripts_and_bad_icons() {
        assert!(upsert_payload(&Some(r#"{"script":"  "}"#.to_owned())).is_err());
        assert!(upsert_payload(&Some(r#"{"script":"x","icon":"banana"}"#.to_owned())).is_err());
        assert!(upsert_payload(&None).is_err());
    }

    #[test]
    fn help_and_schema_state_the_explicit_request_contract() {
        let schema = serde_json::to_string(&schema()).unwrap();
        for text in [USAGE, &schema] {
            assert!(
                text.contains("explicitly asked"),
                "the agent contract must appear in every surface"
            );
            assert!(
                text.contains("no per-call approval"),
                "the absence of an approval gate must be documented"
            );
        }
        // The schema stays machine-readable.
        let _: serde_json::Value = serde_json::from_str(&schema).unwrap();
    }
}
