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
//! `ask` blocks on the human's answer to a structured question shown in
//! their client — reach for it when their decision must come back before
//! you can proceed, not for questions a reply can carry.
//! There is no per-call approval gate; the daemon stamps every accepted
//! write with this task's id so agent-originated changes stay visible to the
//! user.

mod resources;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context as _, anyhow, bail};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use waku_client::DaemonClient;
use waku_protocol::computer_use::ComputerUseRunRequest;
use waku_protocol::custom_commands::{CustomCommand, CustomCommandIcon};
use waku_protocol::model::{ProjectMapIntent, ProviderKind, UserInputOption, UserInputQuestion};
use waku_protocol::{
    AGENT_TASK_ENV, AGENT_TOKEN_ENV, AgentPromptDelivery, AgentWorkspace, Command,
    DAEMON_ADDRESS_ENV, ResponsePayload,
};

const USAGE: &str = "\
goddard-agent — Goddard's scoped agent surface inside a session

USAGE
    goddard-agent create --project PATH --file PROMPT.md
    goddard-agent prompt '<json>'            Send a prompt to an existing task
    goddard-agent rename '<json>'            Rename this task after the user approves the request
    goddard-agent archive '<json>'           Propose archiving tasks after the user approves the request
    goddard-agent read '<json>'              Read a task's transcript
    goddard-agent search --text QUERY         Search task transcripts — this project's, or every project's for the boss
    goddard-agent map --text QUESTION         Find relevant code in this workspace
    goddard-agent merge submit               Rebase, verify, and land this employee worktree
    goddard-agent memory overview|scan|zoom|record|summary|buckets
        This session's shared project memory — omit the bucket to use it
    goddard-agent ask '<json>'               Ask the user a structured question
    goddard-agent computer js '<json>'       Execute JavaScript in this task's persistent CUA kernel
    goddard-agent computer js --stdin        Read that JSON payload from stdin
    goddard-agent computer run '<json>'      Run a bounded Jev-selected browser task
    goddard-agent computer run --stdin       Read that JSON payload from stdin
    goddard-agent computer reset             Reset this task's CUA kernel
    goddard-agent resource acquire '<json>'   Acquire resources (waits, prints id)
    goddard-agent resource run '<json>' -- COMMAND [ARGS]   Run with supervised resources
    goddard-agent resource release '{\"id\":\"UUID\"}'  Release a reservation
    goddard-agent resource cancel '{\"id\":\"UUID\"}'   Cancel a queue entry or workload
    goddard-agent resource status            Print host owners, queue, capacity, and observations
    goddard-agent boss summon --persona NAME --title TITLE --file BRIEF.md
    goddard-agent boss prompt EMPLOYEE_ID --file BRIEF.md
    goddard-agent boss transcript EMPLOYEE_ID [--turn N]
    goddard-agent boss resume EMPLOYEE_ID     Revive an expired employee — transcript and worktree intact
    goddard-agent boss roster [--all]
    goddard-agent read [TASK_ID] [--turn N]
    goddard-agent prompt TASK_ID (--text TEXT | --file PATH|-)
    goddard-agent models                     List the provider/model options `create` accepts
    goddard-agent command list               List the user's custom commands
    goddard-agent command upsert '<json>'    Add or update a custom command
    goddard-agent command remove '<json>'    Remove a custom command
    goddard-agent schema                     Print the JSON payload schemas
    goddard-agent --help                     Show this text

USAGE CONTRACT
    `boss` exposes role-scoped Boss operations. Only the boss or a human
    can edit personas and Boss files; employees read only granted memory
    and knowledge. Delegation is permitted only by the assigned persona.
    `boss script --file` is an advanced boss-only capability: each Rhai
    invocation uses a fresh scope and can batch authorized operations.
    Bosses should delegate execution and long-running commands
    to employees, then verify committed work in their
    worktrees before reporting completion. Respect user-set model and
    resource limits.
    `command` manages the user's settings — today their custom commands —
    and is available whenever changing a setting would help them.
    `create`, `prompt`, and foreign `read` are the cross-task surface. When
    the human asks you to create, start, or spawn another task or session —
    including running work in a separate task — use `create`; when they ask
    you to send a message to another task, use `prompt`. `read` is the read
    half of that surface — it reads a task's transcript, title, provider,
    and status. With no address fields it reads this task's own data; use it
    when another task's transcript holds context you need, for example when
    GODDARD_PARENT_TASK_ID names the task this session is a side chat of.
    `search` is read-only within this task's project — daemon-wide for the
    boss, whose `project:` filters may name any registered project; use it
    to find tasks worth `read`ing. Use `create` and `prompt` only when the
    human you are working for has explicitly asked — never for exploration,
    convenience, or self-orchestration.
    `map` searches this workspace's indexed declarations for code relevant to
    the current task. Ask a specific question, add `anchors` for known symbol
    names, and use `known_paths` when you have already inspected files; then
    read the returned source locations before drawing conclusions. Narrow with
    `path` when you know the relevant directory.
    `memory` reads and records durable notes in this session's shared project
    bucket — `overview` is the compacted entry point, `scan` and `zoom` drill
    into notes, `record` appends one, `summary` answers a pending compression
    request, `buckets` lists every bucket the session can see. Omit the bucket
    to use the project bucket; `--project` addresses another registered
    project when its bucket is granted. Bucket `create` and `migrate` are
    Boss-only and stay under `boss memory`.
    `rename` changes only this task's title. Before deciding, read this task's
    transcript with `goddard-agent read '{}'` to see its current title. Keep
    that title unless the task has substantially changed or pivoted. If
    renaming, preserve its unique subject and describe the task's purpose, not
    recent steps or progress. Unless the task already granted standing
    permission, each call asks the user first — it blocks on the request card
    and fails when the user declines.
    `archive` proposes archiving tasks by Goddard task id — siblings in this
    task's project, found through `search`. It is a proposal, not an action:
    each call renders a request card naming the tasks and your reason, blocks
    until the user answers, and fails when they decline. Nothing is archived
    without that approval. Use it when the human asked for cleanup or a
    task's work is clearly finished — never for exploration or
    self-orchestration.
    `ask` renders a question card in the user's Goddard client and blocks
    until they answer, clarify, or dismiss it. Use it when the human's
    decision — a choice between options or a confirmation — must come back
    before you can proceed; it is not a substitute for ordinary questions
    you can just ask in your reply.
    `models` lists the provider/model combinations `create` accepts, in
    preference order — read it instead of guessing model ids.
    `computer js`, `computer run`, and `computer reset` operate only on this
    task's enabled Computer Use runtime. JavaScript bindings persist; emitted
    images return file paths to open with your image reader. `computer run`
    requires an explicit URL and goal, and can receive field values plus
    machine-checkable completion conditions. Jev sees redacted page context
    and candidate ids; executable browser refs and supplied values stay in the
    daemon. Missing or ambiguous values return a handoff to the parent agent.
    Browser runs use a new isolated profile and close their browser session
    when finished. App, browser, clipboard, and desktop access keep their
    Goddard approval prompts. Actions are never automatically replayed after
    a lost response.
    `resource` reserves contended host resources — native builds, iOS/Android
    virtual devices, and shared desktop input — across every Goddard task on
    this machine. Wrap the workload in `resource run '<json>' -- COMMAND` so
    the reservation queues without retries and supervises the command's
    process group. `resource status` shows owners, queues, and user-owned
    devices. Enforcement is cooperative; commands launched outside a
    reservation bypass the broker.
    There is no per-call approval gate for task/settings writes; the daemon records
    this task's id on every accepted write, so agent-originated commands and
    turns are visibly attributed to it.

ENVIRONMENT
    GODDARD_DAEMON_ADDRESS   Daemon WebSocket address (injected by the daemon)
    GODDARD_AGENT_TOKEN      Per-session scoped credential (injected)
    GODDARD_TASK_ID          This session's task id (injected)
    GODDARD_PARENT_TASK_ID   Parent task id — side-chat sessions only (injected)

Run `goddard-agent schema` for the accepted payloads.";

static JSON_OUTPUT: AtomicBool = AtomicBool::new(false);

macro_rules! println {
    ($($arg:tt)*) => {{
        let value = format!($($arg)*);
        let output = if JSON_OUTPUT.load(Ordering::Relaxed) { value.clone() } else { text_output(&value) };
        std::println!("{output}");
    }};
}

fn text_output(value: &str) -> String {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(value) else {
        return value.to_owned();
    };
    fn format_value(value: &serde_json::Value, depth: usize) -> String {
        let indent = "  ".repeat(depth);
        match value {
            serde_json::Value::Object(map) => map
                .iter()
                .map(|(key, value)| format!("{indent}{key}: {}", format_value(value, depth + 1)))
                .collect::<Vec<_>>()
                .join("\n"),
            serde_json::Value::Array(items) => items
                .iter()
                .map(|item| format!("{indent}- {}", format_value(item, depth + 1)))
                .collect::<Vec<_>>()
                .join("\n"),
            serde_json::Value::String(text) => text.clone(),
            serde_json::Value::Null => "null".into(),
            other => other.to_string(),
        }
    }
    format_value(&json, 0)
}

fn discovery(args: &[String]) -> anyhow::Result<bool> {
    if args.first().is_some_and(|arg| arg == "schema") {
        match &args[1..] {
            [] => {
                let paths = schema()["commands"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>();
                std::println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({"commands":paths}))?
                );
            }
            [all] if all == "--all" => {
                std::println!("{}", serde_json::to_string_pretty(&schema())?)
            }
            _ => bail!("usage: goddard-agent schema [--all]"),
        }
        return Ok(true);
    }
    let content_options = ["--text", "--file", "--json", "--json-file"];
    let Some((marker, index)) = args.iter().enumerate().find_map(|(i, arg)| {
        let is_content = i > 0 && content_options.contains(&args[i - 1].as_str());
        (!is_content && matches!(arg.as_str(), "--help" | "-h" | "--schema"))
            .then_some((arg.as_str(), i))
    }) else {
        return Ok(false);
    };
    let command_index = (1..=index)
        .rev()
        .find(|end| schema()["commands"].get(args[..*end].join(" ")).is_some());
    let Some(command_index) = command_index else {
        let attempted = args[..index].join(" ");
        bail!("unknown command `{attempted}`; run `goddard-agent schema` to list command paths");
    };
    let path = args[..command_index].join(" ");
    let leaf = leaf_schema(&path);
    if marker == "--schema" {
        std::println!("{}", serde_json::to_string_pretty(&leaf)?);
    } else {
        println!(
            "{path}\n\n{}\n\nSchema: goddard-agent {path} --schema",
            leaf["help"]
                .as_str()
                .unwrap_or("Use the documented flags for this command.")
        );
    }
    Ok(true)
}

fn leaf_schema(path: &str) -> serde_json::Value {
    let providers = ProviderKind::ALL
        .iter()
        .copied()
        .map(ProviderKind::id)
        .collect::<Vec<_>>();
    let employee_icons = CustomCommandIcon::EMPLOYEE
        .iter()
        .filter_map(|icon| serde_json::to_value(icon).ok()?.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let command_icons = CustomCommandIcon::ALL
        .iter()
        .filter_map(|icon| serde_json::to_value(icon).ok()?.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let (inputs, output, example) = match path {
        "create" => (
            json!({"--project":{"required":true,"type":"absolute project path"},"--text|--file":{"required":true,"exactlyOne":true,"type":"raw UTF-8 prompt"},"--workspace":{"enum":["local","worktree"],"default":"local"},"--base-branch":{"requiredWhen":"workspace=worktree"},"--provider":{"enum":providers,"default":"inherits current provider"},"--model":"optional; inherits current model","--effort":"optional; inherits current effort","--title":"optional task title","--service-tier":"optional provider service tier","--context-window":"optional context window"}),
            json!({"json":{"task_id":"UUID of the created task"}}),
            "goddard-agent create --project /abs/project --file prompt.md".to_owned(),
        ),
        "search" => (
            json!({"--text|--file":{"required":true,"exactlyOne":true,"type":"literal UTF-8 query"},"--last-turns":{"type":"positive integer","optional":true}}),
            json!({"json":{"results":"matching task/message records","session_link_hint":"how to format Goddard task links"}}),
            "goddard-agent search --text 'status:idle retry logic'".to_owned(),
        ),
        "prompt" => (
            json!({"TASK_ID":{"positional":true,"required":true,"type":"UUID"},"--text|--file":{"required":true,"exactlyOne":true,"type":"raw UTF-8 prompt"},"--delivery":{"enum":["interrupt","queue","steer"],"default":"interrupt"}}),
            json!({"json":{"ok":"true when the prompt was accepted"}}),
            "goddard-agent prompt TASK_ID --file followup.md".to_owned(),
        ),
        "read" => (
            json!({"TASK_ID":{"positional":true,"optional":true,"type":"UUID; omitted reads this task"},"--turn":{"type":"positive 1-based turn number","optional":true}}),
            json!({"json":"AgentSessionTranscript for the addressed task or current task"}),
            "goddard-agent read TASK_ID --turn 3".to_owned(),
        ),
        "boss summon" => (
            json!({"--persona":{"required":true,"type":"exact visible persona name or UUID"},"--title":{"required":true,"type":"job title"},"--text|--file":{"required":true,"exactlyOne":true,"type":"raw UTF-8 prompt"},"--icon":{"enum":employee_icons,"optional":true},"--workspace":{"enum":["local","worktree","adopt"],"default":"local"},"--base-branch":{"requiredWhen":"workspace=worktree"},"--adopt-worktree":{"requiredWhen":"workspace=adopt; worktree address"},"--work-goal":{"enum":["errand","goal"],"default":"errand"},"--plan":{"type":"plan id, planning-session id, or plans/<file>.md — tags the assignment to the plan's Goals group","optional":true},"--item":{"type":"work item UUID inside --plan; requires --plan","optional":true},"--project":{"type":"absolute path","default":"current project; required if none is assigned"},"--provider":{"enum":providers,"optional":"inherits current provider"},"--model":"optional; inherits current model","--effort":"optional; inherits current reasoning effort","--request-id":"optional UUID idempotency key"}),
            json!({"json":{"type":"summoned","sessionId":"employee UUID","state":"queued|dispatching|working","admission":{"provider":"resolved provider","model":"resolved model","reasoningEffort":"resolved effort or null","queuePosition":"optional queue position","blockedBy":"admission blockers"}}}),
            "goddard-agent boss summon --persona Researcher --title 'Review diff' --file brief.md"
                .to_owned(),
        ),
        "boss prompt" => (
            json!({"EMPLOYEE_ID":{"positional":true,"required":true,"type":"UUID"},"--text|--file":{"required":true,"exactlyOne":true,"type":"raw UTF-8 prompt"},"--delivery":{"enum":["interrupt","queue","steer"],"default":"interrupt"}}),
            json!({"json":"BossResult::Saved after the employee accepts the queued prompt or steer"}),
            "goddard-agent boss prompt EMPLOYEE_ID --file followup.md".to_owned(),
        ),
        "boss transcript" => (
            json!({"EMPLOYEE_ID":{"positional":true,"required":true,"type":"UUID"},"--turn":{"type":"positive 1-based turn number","optional":true}}),
            json!({"json":{"type":"transcript","transcript":"AgentSessionTranscript"}}),
            "goddard-agent boss transcript EMPLOYEE_ID --turn 3".to_owned(),
        ),
        "boss roster" => (
            json!({"--all":"include finished employees"}),
            json!({"default":{"type":"roster","roster":"compact active employee roster"},"--all":{"employees":"employee IDs, names, titles, lifecycle, and work goal"}}),
            "goddard-agent boss roster".to_owned(),
        ),
        "boss view" => (
            json!({}),
            json!({"json":{"type":"state","state":"BossState including identity, employees, personas, deliverables, plans, and resource policy"}}),
            "goddard-agent boss view".to_owned(),
        ),
        "boss context" => (
            json!({}),
            json!({"json":{"type":"context","context":"bounded project, task, and automation digest"}}),
            "goddard-agent boss context".to_owned(),
        ),
        "boss stop" => (
            json!({"EMPLOYEE_ID":{"positional":true,"required":true,"type":"UUID"}}),
            json!({"json":"BossResult::Saved after the employee is stopped"}),
            "goddard-agent boss stop EMPLOYEE_ID".to_owned(),
        ),
        "boss resume" => (
            json!({"EMPLOYEE_ID":{"positional":true,"required":true,"type":"UUID"}}),
            json!({"json":"BossResult::Saved after the expired employee requeues"}),
            "goddard-agent boss resume EMPLOYEE_ID".to_owned(),
        ),
        "boss employee rename" => (
            json!({"EMPLOYEE_ID":{"positional":true,"required":true,"type":"UUID"},"NAME":{"positional":true,"required":true,"type":"employee display name"}}),
            json!({"json":{"type":"state","state":"updated BossState"}}),
            "goddard-agent boss employee rename EMPLOYEE_ID 'API reviewer'".to_owned(),
        ),
        "boss employee icon" => (
            json!({"EMPLOYEE_ID":{"positional":true,"required":true,"type":"UUID"},"ICON":{"positional":true,"enum":employee_icons,"requiredUnless":"--clear"},"--clear":{"type":"boolean","requiredUnless":"ICON"}}),
            json!({"json":{"type":"state","state":"updated BossState"}}),
            "goddard-agent boss employee icon EMPLOYEE_ID search".to_owned(),
        ),
        "boss employee model" => (
            json!({"EMPLOYEE_ID":{"positional":true,"required":true,"type":"UUID"},"--provider":{"required":true,"enum":providers},"--model":{"required":true,"type":"provider model ID"},"--effort":{"type":"provider reasoning effort","optional":true},"notes":"one atomic reconfigure — an open turn is interrupted intentionally (never marked failed), the selection applies, and the employee resumes its assignment; a provider+model change re-enters admission against the new pool"}),
            json!({"json":{"type":"saved"}}),
            "goddard-agent boss employee model EMPLOYEE_ID --provider codex --model gpt-5".to_owned(),
        ),
        "boss employee permissions" => (
            json!({"EMPLOYEE_ID":{"positional":true,"required":true,"type":"UUID"},"--json|--json-file":{"required":true,"exactlyOne":true,"object":{"bucketIds":{"type":"string[]","optional":true,"notes":"Boss-created bucket IDs; project bucket access is automatic"},"integrationIds":{"type":"string[]","optional":true},"summonEmployees":{"type":"boolean","optional":true},"computerUse":{"type":"boolean","optional":true}}}}),
            json!({"json":{"type":"saved"}}),
            "goddard-agent boss employee permissions EMPLOYEE_ID --json '{\"bucketIds\":[\"operating-rules\"]}'".to_owned(),
        ),
        "boss employee plan" => (
            json!({"EMPLOYEE_ID":{"positional":true,"required":true,"type":"UUID"},"--json|--json-file":{"required":true,"exactlyOne":true,"object":{"plan":{"type":"plan id, planning-session id, or plans/<file>.md; null clears the tag","optional":true},"item":{"type":"work item UUID inside the plan; null drops to unallocated","optional":true}}}}),
            json!({"json":{"type":"saved"}}),
            "goddard-agent boss employee plan EMPLOYEE_ID --json '{\"plan\":\"plans/session.md\"}'".to_owned(),
        ),
        "boss employee workspace" => (
            json!({"EMPLOYEE_ID":{"positional":true,"required":true,"type":"UUID"},"--json|--json-file":{"required":true,"exactlyOne":true,"object":{"workspace":{"required":true,"enum":["local","worktree"]},"baseBranch":{"requiredWhen":"workspace=worktree"}}}}),
            json!({"json":{"type":"saved","stateChange":"employee resumes in the selected workspace"}}),
            "goddard-agent boss employee workspace EMPLOYEE_ID --json-file workspace.json".to_owned(),
        ),
        "boss employee resources" => (
            json!({"EMPLOYEE_ID":{"positional":true,"required":true,"type":"UUID"},"--json|--json-file":{"required":true,"exactlyOne":true,"object":{"exclusive":{"type":"string[]","default":[]},"resident_devices":{"type":"integer","default":0},"native_builds":{"type":"integer","default":0},"desktop_input":{"type":"integer","default":0}}}}),
            json!({"json":{"type":"saved"}}),
            "goddard-agent boss employee resources EMPLOYEE_ID --json-file resources.json".to_owned(),
        ),
        "boss persona list" => (
            json!({}),
            json!({"json":"array of visible BossPersona records: id, name, markdown, pinnedFiles, permissions, and optional icon"}),
            "goddard-agent boss persona list".to_owned(),
        ),
        "boss persona show" => (
            json!({"ID|NAME":{"positional":true,"required":true,"type":"exact persona UUID or name"}}),
            json!({"json":"array containing the single matching BossPersona record"}),
            "goddard-agent boss persona show Reviewer".to_owned(),
        ),
        "boss script" => (
            json!({"--text|--file":{"required":true,"exactlyOne":true,"accepts":"literal Rhai source; --file accepts PATH or - for stdin","maxBytes":262144,"scope":"fresh per invocation"}}),
            json!({"json":{"type":"eval","value":"Rhai return value (null for unit)","output":"captured print/debug output"}}),
            "goddard-agent boss script --file script.rhai".to_owned(),
        ),
        "boss file write" => (
            json!({"PATH":{"positional":true,"required":true,"type":"Boss-files relative path"},"--text|--file":{"required":true,"exactlyOne":true,"type":"raw UTF-8 content; an empty file clears content"}}),
            json!({"json":{"type":"saved"}}),
            "goddard-agent boss file write plans/auth.md --file auth.md".to_owned(),
        ),
        "boss file list" => (
            json!({"PATH":{"positional":true,"optional":true,"default":"Boss files root","type":"Boss-files relative directory path"}}),
            json!({"json":{"type":"files","files":"array of {path: string, directory: boolean}"}}),
            "goddard-agent boss file list plans".to_owned(),
        ),
        "boss file read" => (
            json!({"PATH":{"positional":true,"required":true,"type":"Boss-files relative path"}}),
            json!({"json":{"type":"file","path":"resolved relative path","content":"UTF-8 file contents"}}),
            "goddard-agent boss file read plans/auth.md".to_owned(),
        ),
        "boss file mkdir" => (
            json!({"PATH":{"positional":true,"required":true,"type":"Boss-files relative directory path"}}),
            json!({"json":{"type":"saved"}}),
            "goddard-agent boss file mkdir plans/archive".to_owned(),
        ),
        "boss persona upsert" => (
            json!({"forms":"Use either --name with --text|--file or one --json|--json-file configuration; do not combine forms","--new|--id":{"required":"exactly one; --new creates with a daemon-assigned UUID"},"--name":{"requiredUnless":"--json|--json-file"},"--text|--file":{"requiredUnless":"--json|--json-file","exactlyOne":true,"type":"persona Markdown"},"--json|--json-file":{"requiredUnless":"--name and --text|--file","exactlyOne":true,"object":{"name":{"required":true,"type":"string"},"markdown":{"required":true,"type":"string"},"pinnedFiles":{"type":"string[]","default":[]},"permissions":{"default":"empty grants","fields":{"bucketIds":"string[] of existing Boss-created buckets","integrationIds":"string[]","summonEmployees":"boolean","computerUse":"boolean"}},"icon":{"enum":employee_icons,"nullable":true,"optional":"omitted preserves on update; null clears"}}}}),
            json!({"json":{"type":"state","state":"updated BossState including the persona record"}}),
            "goddard-agent boss persona upsert --new --name Reviewer --file persona.md".to_owned(),
        ),
        "boss memory buckets" => (
            json!({}),
            json!({"json":{"type":"memory","buckets":"bucket names and purposes visible to this caller; contents are not loaded"}}),
            "goddard-agent boss memory buckets".to_owned(),
        ),
        "boss memory create" => (
            json!({"--json|--json-file":{"required":true,"exactlyOne":true,"object":{"name":{"required":true},"purpose":{"optional":true}}}}),
            json!({"json":{"type":"memory","buckets":"created bucket metadata"}}),
            "goddard-agent boss memory create --json '{\"name\":\"Operating rules\",\"purpose\":\"Boss-owned guidance\"}'".to_owned(),
        ),
        "boss memory overview" => (
            json!({"BUCKET":{"positional":true,"required":"optional — omit with --project or to use the caller's project bucket","type":"named bucket ID"},"--project":{"optional":true,"type":"registered project name/id or absolute project root; resolves to the project's shared bucket"}}),
            json!({"json":{"type":"memory","overview":"bounded summaries and recent original notes","compression":"optional agent-written summary request"}}),
            "goddard-agent boss memory overview --project octane".to_owned(),
        ),
        "boss memory record" => (
            json!({"--json|--json-file":{"required":true,"exactlyOne":true,"object":{"bucket":{"optional":"bucket id; omit with project or to use the caller's project bucket"},"project":{"optional":true,"type":"registered project name/id or absolute project root"},"kind":{"required":true,"enum":["fact","observation","question"]},"text":{"required":true},"retryKey":{"required":true}}},"--project":{"optional":true,"type":"registered project name/id or absolute project root; injected as the payload's project"}}),
            json!({"json":{"type":"memory","recorded":"original note","compression":"optional agent-written summary request"}}),
            "goddard-agent boss memory record --project octane --json-file note.json".to_owned(),
        ),
        "boss memory summary" => (
            json!({"--json|--json-file":{"required":true,"exactlyOne":true,"object":{"bucket":{"optional":"bucket id; omit with project or to use the caller's project bucket"},"project":{"optional":true,"type":"registered project name/id or absolute project root"},"start":{"required":true,"type":"integer note index"},"end":{"required":true,"type":"integer note index"},"text":{"required":true,"type":"agent-written summary"}}},"--project":{"optional":true,"type":"registered project name/id or absolute project root; injected as the payload's project"}}),
            json!({"json":{"type":"memory","compression":"next summary request, if one is ready"}}),
            "goddard-agent boss memory summary --json-file summary.json".to_owned(),
        ),
        "boss memory scan" => (
            json!({"BUCKET":{"positional":true,"required":"optional — omit with --project or to use the caller's project bucket","type":"named bucket ID"},"QUERY":{"positional":true,"required":true,"type":"literal search in original notes"},"--project":{"optional":true,"type":"registered project name/id or absolute project root; resolves to the project's shared bucket"}}),
            json!({"json":{"type":"memory","notes":"matching original notes"}}),
            "goddard-agent boss memory scan --project octane authentication".to_owned(),
        ),
        "boss memory zoom" => (
            json!({"BUCKET":{"positional":true,"required":"optional — omit with --project or to use the caller's project bucket","type":"named bucket ID"},"START":{"positional":true,"required":true,"type":"first note index"},"END":{"positional":true,"required":true,"type":"last note index"},"--project":{"optional":true,"type":"registered project name/id or absolute project root; resolves to the project's shared bucket"}}),
            json!({"json":{"type":"memory","notes":"two child summaries or the original note"}}),
            "goddard-agent boss memory zoom --project octane 1 8".to_owned(),
        ),
        "boss memory migrate" => (
            json!({"BUCKET":{"positional":true,"required":true,"type":"existing named bucket ID"},"SOURCE":{"positional":true,"required":true,"type":"'boss' or absolute project root"},"--dry-run":{"type":"boolean","default":true,"notes":"false appends candidates; source files are never modified"}}),
            json!({"json":{"type":"memory","migration":{"dryRun":"candidate list when true; imported count when false","candidates":"inspectable source paths and original note text"}}}),
            "goddard-agent boss memory migrate project-abc boss".to_owned(),
        ),
        "memory buckets" => (
            json!({}),
            json!({"json":{"type":"memory","buckets":"bucket names and purposes visible to this caller; contents are not loaded"}}),
            "goddard-agent memory buckets".to_owned(),
        ),
        "memory overview" => (
            json!({"BUCKET":{"positional":true,"required":"optional — omit with --project or to use this session's project bucket","type":"named bucket ID"},"--project":{"optional":true,"type":"registered project name/id or absolute project root; resolves to the project's shared bucket"}}),
            json!({"json":{"type":"memory","overview":"bounded summaries and recent original notes","compression":"optional agent-written summary request"}}),
            "goddard-agent memory overview".to_owned(),
        ),
        "memory record" => (
            json!({"--json|--json-file":{"required":true,"exactlyOne":true,"object":{"bucket":{"optional":"bucket id; omit with project or to use this session's project bucket"},"project":{"optional":true,"type":"registered project name/id or absolute project root"},"kind":{"required":true,"enum":["fact","observation","question"]},"text":{"required":true},"retryKey":{"required":true}}},"--project":{"optional":true,"type":"registered project name/id or absolute project root; injected as the payload's project"}}),
            json!({"json":{"type":"memory","recorded":"original note","compression":"optional agent-written summary request"}}),
            "goddard-agent memory record --json-file note.json".to_owned(),
        ),
        "memory summary" => (
            json!({"--json|--json-file":{"required":true,"exactlyOne":true,"object":{"bucket":{"optional":"bucket id; omit with project or to use this session's project bucket"},"project":{"optional":true,"type":"registered project name/id or absolute project root"},"start":{"required":true,"type":"integer note index"},"end":{"required":true,"type":"integer note index"},"text":{"required":true,"type":"agent-written summary"}}},"--project":{"optional":true,"type":"registered project name/id or absolute project root; injected as the payload's project"}}),
            json!({"json":{"type":"memory","compression":"next summary request, if one is ready"}}),
            "goddard-agent memory summary --json-file summary.json".to_owned(),
        ),
        "memory scan" => (
            json!({"BUCKET":{"positional":true,"required":"optional — omit with --project or to use this session's project bucket","type":"named bucket ID"},"QUERY":{"positional":true,"required":true,"type":"literal search in original notes"},"--project":{"optional":true,"type":"registered project name/id or absolute project root; resolves to the project's shared bucket"}}),
            json!({"json":{"type":"memory","notes":"matching original notes"}}),
            "goddard-agent memory scan authentication".to_owned(),
        ),
        "memory zoom" => (
            json!({"BUCKET":{"positional":true,"required":"optional — omit with --project or to use this session's project bucket","type":"named bucket ID"},"START":{"positional":true,"required":true,"type":"first note index"},"END":{"positional":true,"required":true,"type":"last note index"},"--project":{"optional":true,"type":"registered project name/id or absolute project root; resolves to the project's shared bucket"}}),
            json!({"json":{"type":"memory","notes":"two child summaries or the original note"}}),
            "goddard-agent memory zoom 1 8".to_owned(),
        ),
        "boss plan create" => (
            json!({"--title":{"required":true,"type":"plan display title"},"--plan-file":{"required":true,"type":"relative path under plans/"},"--text|--file":{"required":true,"exactlyOne":true,"type":"raw UTF-8 planning brief"},"--provider":{"enum":providers,"optional":true,"default":"codex"},"--model":{"type":"provider model ID","optional":true,"default":"gpt-6.1-sol on codex"},"--effort":{"type":"provider effort ID","optional":true,"default":"medium"}}),
            json!({"json":{"type":"session","session":"planning AgentSession","project":"Boss planning Project"}}),
            "goddard-agent boss plan create --title 'Session redesign' --plan-file session.md --file brief.md".to_owned(),
        ),
        "boss plan finalize" => (
            json!({"PLAN_FILE":{"positional":true,"optional":true,"type":"relative plan path; omitted selects the active plan"},"--items":{"type":"JSON string array","optional":true,"notes":"the approved doc's ordered course of work — seeds the plan's work breakdown"}}),
            json!({"json":{"type":"planFinalized","sessionId":"planning session UUID","planFile":"frozen path under memory/","finalizedAt":"Unix timestamp"}}),
            "goddard-agent boss plan finalize plans/session.md --items '[\"Probe\",\"Verify\"]'".to_owned(),
        ),
        "boss plan items" => (
            json!({"PLAN":{"positional":true,"required":true,"type":"plan id, planning-session id, or plans/<file>.md"},"--json|--json-file":{"required":true,"exactlyOne":true,"type":"ordered item list; entries {\"id\": UUID, \"title\": string} rename/reorder, {\"title\": string} appends, omitted items are dropped"}}),
            json!({"json":{"type":"state","state":"updated BossState"}}),
            "goddard-agent boss plan items plans/session.md --json-file items.json".to_owned(),
        ),
        "boss plan item" => (
            json!({"PLAN":{"positional":true,"required":true,"type":"plan id, planning-session id, or plans/<file>.md"},"ITEM_ID":{"positional":true,"required":true,"type":"work item UUID"},"STATE":{"positional":true,"required":true,"enum":["toDo","done","dropped"],"notes":"toDo reopens a done or dropped item"}}),
            json!({"json":{"type":"state","state":"updated BossState"}}),
            "goddard-agent boss plan item plans/session.md ITEM_ID done".to_owned(),
        ),
        "boss plan outcome" => (
            json!({"PLAN":{"positional":true,"required":true,"type":"plan id, planning-session id, or plans/<file>.md"},"OUTCOME":{"positional":true,"required":true,"enum":["completed","abandoned","approved"],"notes":"approved reopens a closed plan"}}),
            json!({"json":{"type":"state","state":"updated BossState"}}),
            "goddard-agent boss plan outcome plans/session.md completed".to_owned(),
        ),
        "boss deliverable publish" => (
            json!({"PATH":{"positional":true,"required":true,"type":"absolute file or directory path"},"--name":{"type":"optional display name"},"--reference":{"type":"flag; keep a live path instead of copying the file into the daemon's store"}}),
            json!({"json":{"type":"saved"}}),
            "goddard-agent boss deliverable publish /abs/report.md --name Report".to_owned(),
        ),
        "boss deliverable dismiss" => (
            json!({"DELIVERABLE_ID":{"positional":true,"required":true,"type":"UUID"}}),
            json!({"json":{"type":"saved"}}),
            "goddard-agent boss deliverable dismiss DELIVERABLE_ID".to_owned(),
        ),
        "boss speak" => (
            json!({"--text|--file":{"required":true,"exactlyOne":true,"type":"raw UTF-8 speech text"}}),
            json!({"json":{"type":"speak","delivered":"number of client connections notified"}}),
            "goddard-agent boss speak --file briefing.md".to_owned(),
        ),
        "boss report-blocker" => (
            json!({"--text|--file":{"required":true,"exactlyOne":true,"type":"raw UTF-8 blocker message"},"role":"employee only","description":"Report only when you cannot proceed without supervisor or human action: permission denials, missing external state, destructive ambiguity, or genuine product-intent questions after checking repository conventions. Fix recoverable check failures yourself (wrong flags, missing dependencies, flaky retries); resolve style and approach choices from existing code and docs. Put useful non-blocking findings in your finish report. This interrupts your supervisor."}),
            json!({"json":{"type":"saved"}}),
            "goddard-agent boss report-blocker --text 'Blocked: permission denied; supervisor approval required.'".to_owned(),
        ),
        "boss open" => (
            json!({"--provider":{"required":true,"enum":providers},"--model":{"optional":true,"type":"provider model ID"},"--mode":{"enum":["ask","autoAcceptEdits","auto","fullAccess"],"default":"autoAcceptEdits"},"role":"human only"}),
            json!({"json":{"type":"session","session":"Boss AgentSession","project":"Boss Project"}}),
            "goddard-agent boss open --provider codex".to_owned(),
        ),
        "boss browse" => (
            json!({"URL":{"positional":true,"required":true,"type":"http or https URL"}}),
            json!({"json":{"type":"browse","sessionId":"Boss session UUID","url":"opened URL","title":"optional page title"}}),
            "goddard-agent boss browse https://example.com".to_owned(),
        ),
        "boss terminal" => (
            json!({"--title":{"required":true,"type":"terminal tab title"},"--cwd":{"type":"working directory","default":"current directory"},"--file":{"type":"path to UTF-8 startup script","optional":true,"specialPath":"- is read as a literal filename, not stdin"},"role":"Boss only"}),
            json!({"json":{"type":"terminalRequested","title":"terminal tab title","cwd":"working directory"}}),
            "goddard-agent boss terminal --title 'Build log' --cwd /abs/project --file ./watch.sh".to_owned(),
        ),
        "boss rename" => (
            json!({"NAME":{"positional":true,"required":true,"type":"Boss display name"}}),
            json!({"json":{"type":"state","state":"updated BossState"}}),
            "goddard-agent boss rename 'Release assistant'".to_owned(),
        ),
        "boss avatar regenerate" => (
            json!({"EMPLOYEE_ID":{"positional":true,"optional":true,"type":"UUID; omitted regenerates the Boss avatar"}}),
            json!({"json":{"type":"state","state":"updated BossState"}}),
            "goddard-agent boss avatar regenerate EMPLOYEE_ID".to_owned(),
        ),
        "boss automation list" => (
            json!({}),
            json!({"json":{"type":"automations","state":{"automations":"Automation[]","runs":"AutomationRun[]"}}}),
            "goddard-agent boss automation list".to_owned(),
        ),
        "boss automation create" | "boss automation update" => (
            json!({"--json|--json-file":{"required":true,"exactlyOne":true,"object":{"id":{"type":"UUID","requiredWhen":"boss automation update","optionalWhen":"boss automation create"},"name":{"required":true},"prompt":{"required":true},"provider":{"required":true,"enum":providers},"model":{"optional":true},"projectPath":{"required":true,"type":"absolute path"},"workspace":{"enum":["local","worktree","existing"],"default":"local"},"baseBranch":{"requiredWhen":"workspace=worktree"},"sessionId":{"requiredWhen":"workspace=existing"},"schedule":{"optional":true,"discriminator":"kind","variants":{"hourly":{"minute":"0..59"},"daily":{"hour":"0..23","minute":"0..59"},"weekdays":{"hour":"0..23","minute":"0..59"},"weekly":{"dayOfWeek":"0..6, Sunday=0","hour":"0..23","minute":"0..59"},"cron":{"expression":"cron expression"}}},"webhook":{"type":"boolean","default":false},"timezone":{"type":"IANA time zone","optional":true},"enabled":{"type":"boolean","default":false},"precheck":{"optional":true,"fields":{"command":"shell command","timeoutSeconds":{"type":"integer","default":60}}},"missedRunGraceMinutes":{"type":"integer","optional":true},"reuseSession":{"type":"boolean","default":false}}}}),
            json!({"json":{"type":"automations","state":{"automations":"updated Automation[]","runs":"AutomationRun[]"}}}),
            format!("goddard-agent {path} --json-file automation.json"),
        ),
        "boss automation delete" | "boss automation pause" | "boss automation resume" => (
            json!({"AUTOMATION_ID":{"positional":true,"required":true,"type":"UUID"}}),
            json!({"json":{"type":"automations","state":"updated AutomationsState"}}),
            format!("goddard-agent {path} AUTOMATION_ID"),
        ),
        "boss resource-policy show" => (
            json!({}),
            json!({"json":{"type":"state","state":{"resourcePolicy":"BossResourcePolicy"}}}),
            "goddard-agent boss resource-policy show".to_owned(),
        ),
        "boss resource-policy set" => (
            json!({"--json|--json-file":{"required":true,"exactlyOne":true,"object":{"expectedRevision":{"required":true,"type":"current policy revision"},"modelLimits":{"required":true,"items":{"provider":{"required":true,"enum":providers},"model":{"required":true,"type":"provider model ID"},"liveLimit":{"required":true,"type":"integer >= 0"},"hardCap":{"required":true,"type":"integer >= liveLimit"}}},"host":{"optional":true,"fields":{"resident_devices":"integer","native_builds":"integer","desktop_input":"integer"}}}}}),
            json!({"json":{"type":"resourcePolicySet","policy":"accepted BossResourcePolicy with new revision"}}),
            "goddard-agent boss resource-policy set --json-file policy.json".to_owned(),
        ),
        "computer js" => (
            json!({"JSON":{"positional":true,"required":true,"or":"--stdin","object":{"code":{"required":true,"type":"JavaScript source"},"timeout_ms":{"type":"integer milliseconds","optional":true},"title":{"type":"string","optional":true}}}}),
            json!({"json":"Computer Use result object; failures carry isError=true and an error description"}),
            "goddard-agent computer js --stdin".to_owned(),
        ),
        "computer reset" => (
            json!({}),
            json!({"json":{"ok":true}}),
            "goddard-agent computer reset".to_owned(),
        ),
        "resource acquire" => (
            json!({"JSON":{"positional":true,"required":true,"object":{"resources":{"required":true,"fields":{"exclusive":{"type":"string[]","default":[]},"resident_devices":{"type":"integer","default":0},"native_builds":{"type":"integer","default":0},"desktop_input":{"type":"integer","default":0}}},"purpose":{"required":true,"type":"string"},"wait_seconds":{"type":"integer","default":600},"parent":{"type":"reservation UUID","optional":true}}}}),
            json!({"json":{"id":"reservation UUID","borrowed":"whether an enclosing reservation was reused"}}),
            "goddard-agent resource acquire '{\"resources\":{\"native_builds\":1},\"purpose\":\"native build\"}'".to_owned(),
        ),
        "resource run" => (
            json!({"JSON":{"positional":true,"required":true,"object":"same acquisition fields as resource acquire"},"--":{"required":true,"type":"terminates reservation input; followed by executable and arguments"},"COMMAND":{"required":true,"type":"executable and arguments"}}),
            json!({"stdout":"subprocess stdout, passed through","stderr":"subprocess stderr, passed through","exitStatus":"subprocess exit code; reservation released on completion"}),
            "goddard-agent resource run '{\"resources\":{\"native_builds\":1},\"purpose\":\"native build\"}' -- mbx build".to_owned(),
        ),
        "resource release" | "resource cancel" => (
            json!({"JSON":{"positional":true,"required":true,"object":{"id":{"required":true,"type":"reservation UUID"}}}}),
            json!({"json":"ResourceStatus snapshot after release or cancellation"}),
            format!("goddard-agent {path} '{{\"id\":\"RESERVATION_ID\"}}'"),
        ),
        "resource status" => (
            json!({}),
            json!({"json":{"policy":"ResourcePolicy","reservations":"Reservation[]","external_devices":"string[]","observation_errors":"string[]","request_id":"optional UUID","borrowed":"boolean"}}),
            "goddard-agent resource status".to_owned(),
        ),
        "command list" => (
            json!({}),
            json!({"json":"CustomCommand[]"}),
            "goddard-agent command list".to_owned(),
        ),
        "command upsert" => (
            json!({"JSON":{"positional":true,"required":true,"object":{"id":{"type":"UUID","optional":"omit to create"},"name":{"type":"string","optional":true},"icon":{"enum":command_icons,"default":"terminal"},"shell":{"type":"shell path","optional":true},"script":{"required":true,"type":"shell script"},"close_on_success":{"type":"boolean","default":false}}}}),
            json!({"json":"updated CustomCommand[]"}),
            "goddard-agent command upsert '{\"name\":\"Check\",\"script\":\"mbx check -p waku-agent\"}'".to_owned(),
        ),
        "command remove" => (
            json!({"JSON":{"positional":true,"required":true,"object":{"id":{"type":"UUID","optional":true},"name":{"type":"string","optional":true},"constraint":"provide id or name"}}}),
            json!({"json":"updated CustomCommand[]"}),
            "goddard-agent command remove '{\"name\":\"Check\"}'".to_owned(),
        ),
        "merge submit" => (
            json!({}),
            json!({"json":{"landed":"true after the worktree is rebased, verified, and landed","sha":"landed commit SHA"}}),
            "goddard-agent merge submit".to_owned(),
        ),
        "rename" => (
            json!({"JSON":{"positional":true,"required":true,"object":{"title":{"required":true,"type":"this task's new title"}}}}),
            json!({"accepted":{"ok":true},"approvalRequired":"AgentAskOutcome tagged object; invocation waits for the user decision"}),
            "goddard-agent rename '{\"title\":\"Implement CLI schema coverage\"}'".to_owned(),
        ),
        "archive" => (
            json!({"JSON":{"positional":true,"required":true,"object":{"task_ids":{"required":true,"type":"UUID[]"},"reason":{"type":"string","optional":true}}}}),
            json!({"accepted":{"ok":true},"approvalRequired":"AgentAskOutcome tagged object; tasks archive only after explicit approval"}),
            "goddard-agent archive '{\"task_ids\":[\"TASK_UUID\"],\"reason\":\"work is complete\"}'".to_owned(),
        ),
        "ask" => (
            json!({"JSON":{"positional":true,"required":true,"type":"question array or {questions: [...]} object","question":{"id":"optional string","header":"required short label","question":"required prompt","options":"optional array of {label, description?}","multiSelect":"boolean, default false"}}}),
            json!({"json":"AgentAskOutcome tagged object: type=answers with answers[], type=clarified with content, or type=cancelled"}),
            "goddard-agent ask '{\"questions\":[{\"header\":\"Deploy\",\"question\":\"Which environment?\",\"options\":[{\"label\":\"Staging\"}]}]}'".to_owned(),
        ),
        "models" => (
            json!({}),
            json!({"json":{"options":"provider/model options with supported reasoning efforts"}}),
            "goddard-agent models".to_owned(),
        ),
        "map" => (
            json!({"--text|--file":{"required":true,"exactlyOne":true,"type":"literal UTF-8 question"},"--path":{"type":"workspace-relative scope","optional":true},"--intent":{"enum":["locate","understand","change"],"default":"understand"},"--anchors":{"type":"JSON string array","optional":true},"--known-paths":{"type":"JSON path array","optional":true},"--max-tokens":{"type":"integer","minimum":64,"maximum":4096,"optional":true}}),
            json!({"json":"AgentProjectMapResult with ranked source locations and map status"}),
            "goddard-agent map --text 'What controls session expiry?'".to_owned(),
        ),
        _ => unreachable!("missing input schema for executable leaf `{path}`"),
    };
    let mut help = format!("{example}; output defaults to text on a terminal and JSON when piped.");
    if let Some(description) = inputs
        .get("description")
        .and_then(serde_json::Value::as_str)
    {
        help.push(' ');
        help.push_str(description);
    }
    json!({"command":path,"help":help,"inputs":inputs,"outputs":output,"example":example,"globalOptions":["--help","--schema","--output text|json"]})
}

#[allow(dead_code)]
fn legacy_schema() -> serde_json::Value {
    let icons: Vec<String> = CustomCommandIcon::ALL
        .iter()
        .filter_map(|icon| serde_json::to_value(icon).ok()?.as_str().map(str::to_owned))
        .collect();
    let employee_icons: Vec<String> = CustomCommandIcon::EMPLOYEE
        .iter()
        .filter_map(|icon| serde_json::to_value(icon).ok()?.as_str().map(str::to_owned))
        .collect();
    json!({
        "boss": {"description": "Role-scoped Boss operations; payload uses a type tag", "operations": ["view", "summon", "control", "reportBlocker", "transcript", "context", "rename", "renameEmployee", "regenerateAvatar", "upsertPersona", "setEmployeeIcon", "listFiles", "readFile", "writeFile", "createFolder", "publishDeliverable", "dismissDeliverable", "speak", "eval", "memory", "createPlan", "finalizePlan", "setProjectSubmissions", "setProjectQaBranch"], "createPlan": {"title": "the idea the design doc covers — becomes the planning session's title", "planFile": "design doc file name under plans/ in the boss's files root (e.g. \"auth.md\"; accepts plans/<name>.md or workspace/plans/<name>.md spellings)", "prompt": "the user request that prompted planning — seeds the new session's transcript", "provider": "optional provider — defaults to codex", "model": "optional model id — defaults to gpt-6.1-sol when the provider is codex", "reasoningEffort": "optional effort id — defaults to medium", "notes": "boss-only; opens a dedicated boss-attached planning session — a product-design session that drafts a design doc (user experience, flows, edge cases, decisions with rationale; implementation specifics stay out) for the user's approval — once approved, the boss implements the design via employees. Several plans may run at once — reuse an open plan's session instead of starting a second plan on the same idea"}, "finalizePlan": {"planFile": "optional; a planning session omits it to finalize its own plan, the boss chat names the file", "notes": "shows the user an approval card; approval freezes the plan document and reports the approved design to the boss chat — the boss then coordinates implementation via employees, and the planning session archives after a grace period"}, "summon": {"personaId": "UUID assigned by boss", "jobTitle": "purpose-specific job title; Goddard assigns the human name", "prompt": "bounded job", "project": "absolute project path", "workspace": "optional; \"worktree\" runs the employee in a fresh daemon-managed Git worktree, \"adopt\" hands it a finished employee's existing worktree instead of the primary checkout", "baseBranch": "required when workspace is \"worktree\"; ignored for \"adopt\"", "adoptWorktree": "required when workspace is \"adopt\" — absolute path of a daemon-managed worktree whose owning ticket is finished; the employee lands in it with uncommitted state intact", "provider": "optional inherited provider", "model": "optional inherited model", "reasoningEffort": "optional effort id supported by the resolved model — an unsupported id fails the summon", "permissions": "optional per-employee grant overrides — each supplied field (bucketIds, integrationIds, summonEmployees, computerUse) replaces the persona default for this employee, an empty list clears it, omitted fields inherit", "workGoal": "optional; \"errand\" (default) reports the finish to you — use it when the completion feeds your next step; \"goal\" expires silently and lists on the user's Goals page", "icon": format!("optional employee icon enum: {}; overrides the persona icon for this employee only — omit to inherit, and when neither names one the client derives the icon from the job title", employee_icons.join(", ")), "resources": "optional host-resource set reserved for the assignment's lifetime — snake_case keys like resource acquire: {\"exclusive\": [\"ios:<UDID>\"], \"resident_devices\": 1, \"native_builds\": 1, \"desktop_input\": 0}. The employee's own `resource run` calls borrow subsets of it; a contested set queues instead of erroring, and the broker never steals devices the user claimed. Example for a device build: {\"native_builds\": 1}", "allowBurst": "optional boolean; lets the ticket spend burst slots above the model rule's liveLimit — it still cannot pass hardCap. Without it the ticket waits at liveLimit", "groupId": "optional wave id — summons sharing one form a wave: a single notice lands when every member finishes, fails, or is cancelled", "priority": "optional scheduling hint stored on the admission — the FIFO scheduler does not reorder on it yet", "goalId": "optional UUID of a daemon-owned goal the assignment projects onto", "requestId": "optional UUID idempotency key — a retry that lost its response returns the original employee; reusing the id with different fields errors"}, "control": {"sessionId": "employee UUID", "action": {"type": "prompt | steer | stop | setModel | setPermissions | setWorkspace | setResources", "prompt": "required for prompt and steer", "delivery": "optional for prompt — \"interrupt\" (default) steers into the employee's open turn and queues when none is open, \"queue\" parks behind the current work, \"steer\" requires a live turn", "jobTitle": "optional for steer — retitles the job when the steer redirects the assignment; bookkeeping only, it queues no prompt and writes no transcript entry", "provider": "required for setModel", "model": "catalog model id required for setModel", "reasoningEffort": "optional effort id supported by the selected model", "permissions": "required for setPermissions — same per-field override shape as summon; each supplied field replaces the employee's current grant; MCP and Computer Use changes apply to its next launch", "workspace": "required for setWorkspace — \"local\" returns the employee to the project's primary checkout, \"worktree\" forks a fresh daemon-managed worktree", "baseBranch": "required for setWorkspace when workspace is \"worktree\" — the ref the new worktree detaches at; ignored for \"local\"", "resources": "required for setResources — a host-resource set like summon's; a queued ticket re-enters admission on it, a running employee swaps to it once capacity grants without interrupting its turn", "notes": "setWorkspace is one atomic move: it interrupts the employee's current turn, rebinds the session, and resumes the same transcript in the new workspace — a failure leaves it running in its old workspace. setModel is one atomic reconfigure: an open turn is interrupted — recorded as an intentional stop, not a failure — the selection applies, and the employee resumes its assignment on it; a provider+model change re-enters admission against the new pool while a same-pair change retunes the live runtime, and a queued ticket retickets in place"}}, "reportBlocker": {"message": "what needs supervisor attention (<=1000 chars)", "notes": "employee-only; interrupts the supervisor's running turn when it can and makes the finish deliver a full report — flag blockers, needed decisions, and failures, never routine completions"}, "transcript": {"sessionId": "employee UUID", "turn": "optional turn number; omit for index", "notes": "pull-only; a flagged, errand, or always-report employee's index arrives at finish — never poll it"}, "context": {"type": "context", "result": "snapshot of the human's projects, tasks, and automations"}, "files": "document paths are relative to the boss's persistent files root; writeFile/createFolder are boss-only while employees may read pinned documents — memory is managed through named bucket operations, not files or folders", "speak": {"parts": ["ordered utterance fragments, 1-8; each becomes or reuses a canned voice clip", "several parts chain into a sentence — isolate proper nouns and reusable phrases as their own parts so generated audio is reused", "boss-only; returns {\"type\":\"speak\",\"delivered\":<client connections reached>} — 0 means nobody could hear it"], "example": {"type": "speak", "parts": ["Your build on ", "Goddard", " finished"]}},  "publishDeliverable": {"path": "absolute path of an employee-produced file or folder; shows it in the user's sidebar — employees may publish only paths inside their own assigned workspace; the daemon copies the target into its deliverable store so the entry survives the workspace", "name": "optional display name; defaults to the file name", "reference": "optional boolean; true keeps a live filesystem reference instead of copying — for artifacts too large for the store or meant to stay current — and the entry dies with the path"}, "dismissDeliverable": {"id": "deliverable UUID from view"},  "memory": {"operation": {"type": "listBuckets | createBucket | overview | record | submitSummary | scan | zoomBucket", "notes": "content operations pick a bucket three ways: an explicit `bucket` id, a `project` reference — a registered project name/id or an absolute project root — resolving to that project's shared bucket, or no selector at all for the caller's own project bucket (employees); bucket metadata can be listed without loading contents; Boss-created bucket IDs are explicit grants"}}, "setEmployeeIcon": {"sessionId": "employee UUID", "icon": format!("optional custom icon enum: {} (null clears override)", employee_icons.join(", "))},  "eval": {"script": "Rhai source run inside the daemon with the boss operations bound as functions — one call batches operations and chains their results; variables persist between evals for this boss session. Bindings: view() → state map, context() → digest string, summon(#{personaId,jobTitle,prompt,project,...}) → sessionId, control(sessionId, action-map | \"stop\"), transcript(sessionId[, turn]), readFile(path) → #{path,content}, writeFile(path, content), listFiles([path]), createFolder(path), publishDeliverable(path[, name]), dismissDeliverable(id), speak(parts|string) → delivered, upsertPersona(#{name,markdown,...}) — id/pinnedFiles/permissions optional, setEmployeeIcon(sessionId, icon|null), rename(name), renameEmployee(sessionId, name), regenerateAvatar([sessionId]), memory(#{type,...}) → memory store fields, op(#{type,...}) → whole result for any other operation, help() → binding list. Returns {\"type\":\"eval\",\"value\":<script's last expression as JSON>,\"output\":<captured print/debug text>}. Boss-only, bounded by a wall-clock and operations budget.", "example": "let s = view(); s.employees.len()"},  "examples": [{"type": "view"}, {"type": "readFile", "path": "plans/auth.md"}, {"type": "publishDeliverable", "path": "/abs/path/to/output", "name": "Q3 report"}], "setProjectSubmissions": {"project": "registered project name/id or absolute project root", "enabled": "boolean", "notes": "boss/human only; merge submit is off per project until enabled — a worktree employee's `merge submit` fails with 'submissions not enabled for this project' while it is off"}, "setProjectQaBranch": {"project": "registered project name/id or absolute project root", "branch": "branch name the project's merge submit landings and review train use; omit or pass null to clear the override and inherit the daemon-global qa_branch setting", "notes": "boss/human only; the named branch must be checked out somewhere in the project's repository for submissions to land"}, "persona": {"id": "UUID; nil creates a persona", "name": "string", "markdown": "Markdown personality", "pinnedFiles": "document paths beneath the Boss files root pinned into the agent's context; memory bucket access is controlled by bucketIds", "permissions": {"bucketIds": "Boss-created memory bucket IDs", "integrationIds": "connected integration ids", "summonEmployees": "boolean", "computerUse": "boolean", "alwaysReport": "boolean; finishes report to the supervisor instead of expiring silently"}}},
        "merge": {"submit": "no payload; submits this employee's daemon-managed worktree to the configured QA branch after serialized rebase and verification"},
        "computer": {
            "js": {"code": "string (required)", "timeout_ms": "integer 1..300000 (default 300000)", "title": "string (optional)"},
            "run": {"url": "http(s) URL (required)", "goal": "string (required)", "values": "optional object mapping accessible field labels to supplied text", "verify": {"urlContains": "optional string", "textContains": "optional string[]", "fields": "optional object mapping accessible field labels to exact expected values; at least one check required"}, "maxActions": "integer 1..32 (default 12)", "timeoutMs": "integer 1..120000 (default 60000)"},
            "reset": "no payload; resets only this task",
            "images": "content image blocks return local path and mimeType; open each path with your image-reading tool"
        },
        "usage_contract": "`command` manages the user's settings — today their custom commands — and is available whenever changing a setting would help them. `map` searches this workspace's indexed declarations for code relevant to the current task; use a specific question, add symbol names in `anchors`, note already inspected files in `known_paths`, and read the returned source before drawing conclusions. `create` and `prompt` are the cross-task surface: only invoke them when the human you are working for has explicitly asked you to create another task or to send a message to one. `ask` shows the human a structured question and blocks on their answer — use it when their decision must come back before you can proceed, not for questions a reply can carry. `archive` proposes archiving tasks in this task's project — each call shows the user the named tasks and your reason on a request card and blocks on their answer; nothing is archived without approval. There is no per-call approval gate for other task/settings writes. Computer Use retains its app/browser/clipboard/desktop approval gates. Computer run uses Jev to select semantic browser actions and returns needs_input or needs_parent when it cannot safely finish. The daemon records this task's id on every accepted write so agent-originated changes stay visibly attributed.",
        "resource": {
            "syntax": "resource acquire '<json>' | resource run '<json>' -- COMMAND [ARGS] | resource release/cancel '{\"id\":\"UUID\"}' | resource status",
            "acquire": {"resources": {"exclusive": ["ios:SIMULATOR-UDID"], "resident_devices": 1, "native_builds": 1, "desktop_input": 0}, "purpose": "iOS smoke test", "wait_seconds": 600},
            "notes": "FIFO atomic allocation across projects. run inherits standard streams and exit status. Use ios:<UDID>, android:<AVD>, or device:<id> exclusivity; resident_devices must equal ios/android resource count. Nested run inherits subset ownership, cannot expand. Reservations remain while workloads or named devices live. Cooperative enforcement; raw launches bypass scheduling. Boss sessions cannot acquire: the boss delegates waits to employees."
        },
        "create": {
            "description": "Create a fully configured task and immediately start its first prompt. There is no idle-task creation. The task inherits this task's access mode and run environment — a sandboxed task spawns sandboxed tasks.",
            "fields": {
                "provider": {"type": "string", "enum": ["amp", "claude", "codex", "cursor", "deepseek", "devin", "fx", "opencode", "goose", "grok", "kimi", "muse", "ohmypi", "pi"], "notes": "omit to run the new task on this task's provider; run `goddard-agent models` for the usable providers"},
                "model": {"type": "string", "notes": "explicit provider model id — run `goddard-agent models` for the preference-ordered list of usable ids instead of guessing; \"auto\" routes the first prompt through Jev to pick provider and model (omit `provider`); \"default\" selects the provider's own default; omit to inherit this task's model when it runs the resolved provider"},
                "project": {"type": "string", "required": true, "notes": "absolute path; resolves an existing project or registers a primary Git checkout (linked worktrees are rejected)"},
                "workspace": {"type": "string", "required": true, "enum": ["local", "worktree"]},
                "base_branch": {"type": "string", "required_when": "workspace == \"worktree\"", "notes": "ignored for \"local\""},
                "prompt": {"type": "string", "required": true},
                "title": {"type": "string", "notes": "optional explicit task title; when supplied, first-prompt title inference is skipped; when omitted, the title is inferred as usual"},
                "reasoning_effort": {"type": "string", "notes": "provider-specific effort id; \"default\" selects the provider's own default; omit to inherit this task's effort when it runs the resolved provider (falls back to the model's default when the resolved model does not list it)"},
                "service_tier": {"type": "string", "notes": "provider-specific tier id; inherits like reasoning_effort"},
                "context_window": {"type": "string", "notes": "provider-specific window id; inherits like reasoning_effort"}
            },
            "example": "{\"project\":\"/abs/path\",\"workspace\":\"worktree\",\"base_branch\":\"main\",\"prompt\":\"Summarize the diff\",\"title\":\"Summarize the project diff\"}",
            "returns": {"task_id": "uuid of the created task"}
        },
        "prompt": {
            "description": "Submit a prompt to an existing task, addressed by Goddard task id or provider-native thread id. A Boss employee may target only its supervisor task — that is the employee report channel; any other target is rejected with the correct path (reportBlocker for attention, merge submit to land work).",
            "fields": {
                "task_id": {"type": "string", "notes": "Goddard task UUID; exactly one of task_id and thread_id is required"},
                "thread_id": {"type": "string", "notes": "provider-native Agent CLI thread id; exactly one of task_id and thread_id is required"},
                "provider": {"type": "string", "notes": "disambiguates thread_id when several tasks share it"},
                "prompt": {"type": "string", "required": true},
                "delivery": {"type": "string", "enum": ["interrupt", "queue", "steer"], "default": "interrupt", "notes": "interrupt (default) steers into the running turn when one is open and queues otherwise; queue waits for the target to go idle and preserves submission order; steer injects into the running turn and fails when no turn is running"}
            },
            "example": "{\"task_id\":\"<uuid>\",\"prompt\":\"How is the migration going?\",\"delivery\":\"interrupt\"}",
            "returns": {"ok": true}
        },
        "models": {
            "description": "List the provider/model combinations `create` accepts, in preference order — read it instead of guessing model ids. The \"auto\" entry (Jev routing picks provider and model for the first prompt) leads when the eval backend is configured; the rest are combinations tasks on this machine have actually run, each model's first-party (vendor-native) harness ahead of third-party harnesses, then most recently used. Prefer the earliest entry unless the user specified a model. Each option's fields map straight onto `create` payload fields.",
            "returns": {"options": [{"provider": "provider id — absent only on the \"auto\" entry", "model": "model id, \"default\", or \"auto\"", "reasoning_effort": "effort id the newest use ran, when known", "service_tier": "tier id, when known", "context_window": "window id, when known", "last_used_at": "unix seconds; 0 on the \"auto\" entry"}]}
        },
        "rename": {
            "description": "Set this task's title. The user approves each request unless the task already granted standing permission. Cannot rename another task.",
            "fields": { "title": {"type": "string", "required": true} },
            "example": "{\"title\":\"Investigate session startup\"}",
            "returns": {"ok": true}
        },
        "archive": {
            "description": "Propose archiving tasks in this task's project, by Goddard task id. The user sees the named tasks and your reason on a request card and approves or declines — the call blocks until then and fails on decline. Targets must be started, unarchived, and not side chats; archived tasks' side chats leave with them. Use `search` to find task ids.",
            "fields": {
                "task_ids": {"type": "array of strings", "required": true, "notes": "Goddard task UUIDs to archive"},
                "reason": {"type": "string", "notes": "why these tasks should be archived; shown on the request card"}
            },
            "example": "{\"task_ids\":[\"<uuid>\",\"<uuid>\"],\"reason\":\"duplicates of the migration investigation\"}",
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
        "search": {
            "description": "Search the transcripts of every task in this task's project — for the boss, every task on this daemon. `query` is free text — a single case-insensitive substring over user and assistant messages — plus `field:value` filters: `project:<name>` (must name this project; the boss may name any registered project), `status:<idle|connecting|working|waiting|background|failed|busy>` (`busy` unions the working set), `archived:<true|false|any>` (default: active tasks only), `limit:<n>` (default 20). Repeated project:/status: tokens union; different filters intersect; unrecognized tokens stay literal text. A filters-only query lists matching tasks. `last_turns` narrows each task's corpus to its N most recent turns — the units `read` numbers — so a stale hit in an early turn cannot outrank recent work.",
            "fields": {
                "query": {"type": "string", "required": true},
                "last_turns": {"type": "number", "notes": "search only each task's last N turns; omit to scan whole transcripts"}
            },
            "example": "{\"query\":\"status:idle retry logic\"}",
            "returns": {"results": [{"task_id": "uuid", "title": "string", "project": "project name", "provider": "string", "status": "string", "updated_at": "unix seconds", "source": "user|assistant", "message_id": "uuid of the matched message", "snippet": "matched excerpt"}], "session_link_hint": "how to link a task in your reply"}
        },
        "map": {
            "description": "Ask Jev to rank source evidence from this session's indexed workspace, then return relevant declarations and locations. Use for code discovery and query again as you learn more. Requires the Project Map experiment to be enabled.",
            "fields": {
                "query": {"type": "string", "required": true, "notes": "a precise natural-language question about code or behavior"},
                "path": {"type": "string", "notes": "workspace-relative directory or file scope"},
                "intent": {"type": "string", "enum": ["locate", "understand", "change"], "default": "understand"},
                "anchors": {"type": "string[]", "notes": "paths or qualified symbols already known to relate to the query"},
                "known_paths": {"type": "string[]", "notes": "already-read paths; they are deprioritized, never excluded"},
                "max_tokens": {"type": "number", "default": 1024, "minimum": 64, "maximum": 4096}
            },
            "example": "{\"query\":\"What controls how long a login session lasts?\",\"intent\":\"understand\",\"anchors\":[\"src/auth/session.rs\"],\"known_paths\":[\"src/auth/session.rs\"]}",
            "returns": {"query": "string", "intent": "locate|understand|change", "text": "ranked source context with paths and line numbers", "indexed_files": "number", "candidates_considered": "number", "omitted_candidates": "number", "mapped_files": "number", "estimated_tokens": "number", "truncated": "boolean", "ranking": "jev|localFallback", "fallback_reason": "optional reason Jev ranking was unavailable"}
        },
        "ask": {
            "description": "Ask this task's user a structured question and block until they resolve it. Renders the session's question card in their Goddard client while your turn keeps running — use it when a human decision (a choice between options, or a confirmation) must come back before you can proceed. Do not use it for questions an ordinary reply can carry.",
            "fields": {
                "questions": {"type": "array", "required": true, "notes": "one or more questions, presented one card at a time in order", "items": {
                    "question": {"type": "string", "required": true},
                    "header": {"type": "string", "notes": "short card label; defaults to \"Question\""},
                    "id": {"type": "string", "notes": "answer key; defaults to question-<index>"},
                    "options": {"type": "array", "items": {"label": {"type": "string", "required": true}, "description": {"type": "string"}}, "notes": "omit for a free-form answer"},
                    "multiSelect": {"type": "boolean", "default": false}
                }}
            },
            "example": "{\"questions\":[{\"header\":\"Deploy\",\"question\":\"Which environment should I deploy to?\",\"options\":[{\"label\":\"Staging\"},{\"label\":\"Production\",\"description\":\"Requires sign-off\"}]}]}",
            "returns": {"outcome": "{\"type\":\"answers\",\"answers\":[{\"questionId\":\"<id>\",\"answers\":[\"<chosen label or typed text>\"]}]} when the user submits; {\"type\":\"clarified\",\"content\":\"<text>\"} when they explain instead; {\"type\":\"cancelled\"} when they dismiss or the turn ends"}
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

fn schema() -> serde_json::Value {
    let paths = [
        "create",
        "prompt",
        "read",
        "search",
        "map",
        "rename",
        "archive",
        "ask",
        "models",
        "boss summon",
        "boss prompt",
        "boss transcript",
        "boss roster",
        "boss view",
        "boss context",
        "boss stop",
        "boss resume",
        "boss employee rename",
        "boss employee icon",
        "boss employee model",
        "boss employee permissions",
        "boss employee plan",
        "boss employee workspace",
        "boss employee resources",
        "boss persona list",
        "boss persona show",
        "boss persona upsert",
        "boss file list",
        "boss file read",
        "boss file write",
        "boss file mkdir",
        "boss memory buckets",
        "boss memory create",
        "boss memory overview",
        "boss memory record",
        "boss memory summary",
        "boss memory scan",
        "boss memory zoom",
        "boss memory migrate",
        "memory buckets",
        "memory overview",
        "memory record",
        "memory summary",
        "memory scan",
        "memory zoom",
        "boss plan create",
        "boss plan finalize",
        "boss plan items",
        "boss plan item",
        "boss plan outcome",
        "boss deliverable publish",
        "boss deliverable dismiss",
        "boss speak",
        "boss automation list",
        "boss automation create",
        "boss automation update",
        "boss automation delete",
        "boss automation pause",
        "boss automation resume",
        "boss resource-policy show",
        "boss resource-policy set",
        "boss open",
        "boss browse",
        "boss terminal",
        "boss rename",
        "boss avatar regenerate",
        "boss report-blocker",
        "boss script",
        "computer js",
        "computer reset",
        "resource acquire",
        "resource run",
        "resource release",
        "resource cancel",
        "resource status",
        "command list",
        "command upsert",
        "command remove",
        "merge submit",
    ];
    let commands = paths
        .into_iter()
        .map(|path| (path.to_owned(), leaf_schema(path)))
        .collect::<serde_json::Map<_, _>>();
    json!({
        "commands": commands,
        "usage_contract": "Use create or prompt only when the human explicitly asked. There is no per-call approval gate. Every leaf supports local help and schema discovery. Rich configuration stays in command-specific JSON; raw content uses --text or --file."
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
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
    title: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    service_tier: Option<String>,
    #[serde(default)]
    context_window: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
struct SearchPayload {
    query: String,
    /// Restrict matches to each task's last N turns.
    #[serde(default)]
    last_turns: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectMapPayload {
    query: String,
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    max_tokens: Option<usize>,
    #[serde(default)]
    intent: ProjectMapIntent,
    #[serde(default)]
    anchors: Vec<String>,
    #[serde(default)]
    known_paths: Vec<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenamePayload {
    title: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchivePayload {
    task_ids: Vec<Uuid>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
struct CommandRemovePayload {
    #[serde(default)]
    id: Option<Uuid>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PersonaUpsertInput {
    name: String,
    markdown: String,
    #[serde(default)]
    pinned_files: Vec<String>,
    #[serde(default)]
    permissions: waku_protocol::boss::PersonaPermissions,
    #[serde(default, deserialize_with = "deserialize_optional_icon")]
    icon: Option<Option<CustomCommandIcon>>,
}

fn deserialize_optional_icon<'de, D>(
    deserializer: D,
) -> Result<Option<Option<CustomCommandIcon>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<CustomCommandIcon>::deserialize(deserializer).map(Some)
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
    Interrupt,
    Queue,
    Steer,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let message = format!("{error:#}");
            if JSON_OUTPUT.load(Ordering::Relaxed) {
                std::eprintln!(
                    "{}",
                    json!({"error":{"code":"invalid_input","command":"goddard-agent","message":message,"hint":"Run the command with --help or --schema."}})
                );
            } else {
                eprintln!("goddard-agent: {message}");
            }
            ExitCode::from(2)
        }
    }
}

fn run() -> anyhow::Result<()> {
    let mut raw_args = std::env::args().skip(1).collect::<Vec<_>>();
    if raw_args == ["--build-commit"] {
        println!("{}", option_env!("GODDARD_COMMIT_SHA").unwrap_or("unknown"));
        return Ok(());
    }
    let mut explicit_output = None;
    let mut index = 0;
    let value_options = [
        "--text",
        "--file",
        "--json",
        "--json-file",
        "--persona",
        "--title",
        "--icon",
        "--project",
        "--provider",
        "--model",
        "--effort",
        "--work-goal",
        "--workspace",
        "--base-branch",
        "--adopt-worktree",
        "--request-id",
        "--delivery",
        "--turn",
        "--output",
        "--timeout-ms",
        "--cwd",
        "--script",
        "--name",
        "--id",
        "--path",
        "--reason",
        "--url",
    ];
    while index < raw_args.len() {
        if raw_args[index] == "--output" {
            if explicit_output.is_some() {
                bail!("--output may be supplied only once");
            }
            let value = raw_args
                .get(index + 1)
                .ok_or_else(|| anyhow!("--output requires text or json"))?;
            if !matches!(value.as_str(), "text" | "json") {
                bail!("--output must be text or json");
            }
            explicit_output = Some(value.clone());
            raw_args.drain(index..=index + 1);
        } else if value_options.contains(&raw_args[index].as_str()) {
            index = (index + 2).min(raw_args.len());
        } else {
            index += 1;
        }
    }
    JSON_OUTPUT.store(
        explicit_output
            .as_deref()
            .map(|s| s == "json")
            .unwrap_or_else(|| !std::io::stdout().is_terminal()),
        Ordering::Relaxed,
    );
    if discovery(&raw_args)? {
        return Ok(());
    }
    let mut arguments = raw_args.into_iter();
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
        "boss"
            if arguments.clone().next().is_some_and(|s| {
                matches!(
                    s.as_str(),
                    "summon"
                        | "prompt"
                        | "transcript"
                        | "roster"
                        | "script"
                        | "file"
                        | "persona"
                        | "memory"
                        | "plan"
                        | "deliverable"
                        | "speak"
                        | "automation"
                        | "resource-policy"
                        | "open"
                        | "browse"
                        | "terminal"
                        | "rename"
                        | "avatar"
                        | "report-blocker"
                        | "employee"
                        | "context"
                        | "view"
                        | "stop"
                )
            }) =>
        {
            let action = arguments.next().unwrap();
            boss_everyday(&action, arguments.collect())
        }
        "prompt" | "read" => task_everyday(&subcommand, arguments.collect()),
        "create"
            if arguments
                .clone()
                .next()
                .is_some_and(|arg| arg.starts_with("--")) =>
        {
            task_create(arguments.collect())
        }
        "map"
            if arguments
                .clone()
                .next()
                .is_some_and(|arg| arg.starts_with("--")) =>
        {
            task_map(arguments.collect())
        }
        "search"
            if arguments
                .clone()
                .next()
                .is_some_and(|arg| arg.starts_with("--")) =>
        {
            task_search(arguments.collect())
        }
        "models" => {
            if arguments.next().is_some() {
                bail!("`models` takes no arguments");
            }
            match connect()?.request(request_session_id(), Uuid::nil(), Command::AgentListModels)? {
                ResponsePayload::AgentModelOptions { options } => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({ "options": options }))?
                    );
                    Ok(())
                }
                other => bail!("daemon returned an unexpected response: {other:?}"),
            }
        }
        "memory" => task_memory(arguments.collect()),
        "computer" => computer(arguments.collect()),
        "merge" => {
            if arguments.next().as_deref() != Some("submit") || arguments.next().is_some() {
                bail!("usage: goddard-agent merge submit");
            }
            match connect()?.request_with_timeout(
                request_session_id(),
                Uuid::nil(),
                Command::AgentMergeSubmit,
                None,
            )? {
                ResponsePayload::AgentMergeSubmitted { sha } => {
                    println!("{}", json!({ "landed": true, "sha": sha }));
                    Ok(())
                }
                other => bail!("daemon returned an unexpected response: {other:?}"),
            }
        }
        "resource" => resources::command(arguments),
        #[cfg(unix)]
        "__resource_exec" => resources::exec_child(arguments),
        "command" => command(arguments.next().as_deref(), arguments.next()),
        "create" | "search" | "map" | "rename" | "archive" | "ask" | "boss" => {
            let payload = arguments
                .next()
                .ok_or_else(|| anyhow!("`{subcommand}` takes one JSON object argument; run `goddard-agent schema` for its shape"))?;
            if arguments.next().is_some() {
                bail!("`{subcommand}` accepts exactly one JSON object argument");
            }
            let command = build_command(&subcommand, &payload)?;
            let client = connect()?;
            // `ask`, `archive`, and an ungranted `rename` wait on a human —
            // a clock can't bound that, so they park until the daemon
            // resolves them or the connection drops.
            let response = if matches!(subcommand.as_str(), "ask" | "rename" | "archive") {
                client.request_with_timeout(request_session_id(), Uuid::nil(), command, None)?
            } else {
                client.request(request_session_id(), Uuid::nil(), command)?
            };
            match response {
                ResponsePayload::Boss { result } => {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                ResponsePayload::AgentSessionCreated { session_id } => {
                    println!("{}", serde_json::json!({ "task_id": session_id }));
                }
                ResponsePayload::AgentSessionTranscript { transcript } => {
                    println!("{}", serde_json::to_string_pretty(&transcript)?);
                }
                ResponsePayload::AgentSessionSearch { hits } => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "results": hits,
                            "session_link_hint": session_link_hint(),
                        }))?
                    );
                }
                ResponsePayload::AgentProjectMap { result } => {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                ResponsePayload::AgentAskResult { outcome } => {
                    println!("{}", serde_json::to_string_pretty(&outcome)?);
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

/// Parse the small, explicit flag grammar used by the everyday commands.
/// Values are kept verbatim so text read from a file or stdin is never
/// interpreted as shell or JSON syntax.
fn flags(
    args: Vec<String>,
    value_flags: &[&str],
    positional: bool,
) -> anyhow::Result<(Vec<String>, std::collections::BTreeMap<String, String>)> {
    let mut positionals = Vec::new();
    let mut values = std::collections::BTreeMap::new();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if let Some(key) = arg.strip_prefix("--") {
            if matches!(key, "all" | "new" | "clear" | "reference") {
                if values.insert(key.to_owned(), String::new()).is_some() {
                    bail!("`--{key}` may be supplied only once");
                }
                continue;
            }
            if !value_flags.contains(&key) {
                bail!("unknown option `--{key}`");
            }
            let value = iter
                .next()
                .ok_or_else(|| anyhow!("`--{key}` requires a value"))?;
            if values.insert(key.to_owned(), value).is_some() {
                bail!("`--{key}` may be supplied only once");
            }
        } else if positional {
            positionals.push(arg);
        } else {
            bail!("unexpected argument `{arg}`");
        }
    }
    Ok((positionals, values))
}

fn raw_input(values: &std::collections::BTreeMap<String, String>) -> anyhow::Result<String> {
    let text = values.get("text");
    let file = values.get("file");
    if text.is_some() == file.is_some() {
        bail!("provide exactly one of --text or --file PATH|-");
    }
    let body = if let Some(text) = text {
        text.clone()
    } else {
        let path = file.unwrap();
        if path == "-" {
            let mut body = String::new();
            std::io::Read::read_to_string(
                &mut std::io::Read::take(std::io::stdin().lock(), 4 * 1024 * 1024 + 1),
                &mut body,
            )?;
            if body.len() > 4 * 1024 * 1024 {
                bail!("input exceeds 4 MB");
            }
            body
        } else {
            std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?
        }
    };
    Ok(body)
}

fn boss_everyday(action: &str, args: Vec<String>) -> anyhow::Result<()> {
    use waku_protocol::boss::BossOperation;
    match action {
        "view" | "context" | "roster" => {
            if action == "view" {
                if !args.is_empty() {
                    bail!("boss view takes no arguments");
                }
                print_boss(boss_request(BossOperation::View)?)
            } else if action == "context" {
                if !args.is_empty() {
                    bail!("boss context takes no arguments");
                }
                print_boss(boss_request(BossOperation::Context)?)
            } else {
                let (_, opts) = flags(args, &[], false)?;
                if opts.contains_key("all") {
                    let waku_protocol::boss::BossResult::State { state } =
                        boss_request(BossOperation::View)?
                    else {
                        bail!("unexpected Boss view result")
                    };
                    let employees = state.employees.into_iter().map(|e| json!({"id":e.session_id,"name":e.identity.name,"jobTitle":e.job_title,"lifecycle":e.state,"workGoal":e.work_goal})).collect::<Vec<_>>();
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({"employees":employees}))?
                    );
                    Ok(())
                } else {
                    print_boss(boss_request(BossOperation::Roster)?)
                }
            }
        }
        "script" => {
            let (_, opts) = flags(args, &["text", "file"], false)?;
            let script = raw_input(&opts)?;
            if script.is_empty() {
                bail!("script source must not be empty");
            }
            if script.len() > 256 * 1024 {
                bail!("script source exceeds 262144 bytes");
            }
            print_boss(boss_request(BossOperation::Eval { script })?)
        }
        "transcript" => {
            let (pos, opts) = flags(args, &["turn"], true)?;
            if pos.len() != 1 {
                bail!("usage: goddard-agent boss transcript EMPLOYEE_ID [--turn N]");
            }
            let session_id = Uuid::parse_str(&pos[0]).context("invalid employee ID")?;
            let turn = opts
                .get("turn")
                .map(|v| v.parse().context("--turn must be a positive integer"))
                .transpose()?;
            print_boss(boss_request(BossOperation::Transcript {
                session_id,
                turn,
            })?)
        }
        "prompt" => {
            let (pos, opts) = flags(args, &["file", "text", "delivery"], true)?;
            if pos.len() != 1 {
                bail!(
                    "usage: goddard-agent boss prompt EMPLOYEE_ID [--delivery interrupt|queue|steer] (--text TEXT|--file PATH|-)"
                );
            }
            let prompt = raw_input(&opts)?;
            if prompt.trim().is_empty() {
                bail!("prompt must not be empty");
            }
            let delivery = opts
                .get("delivery")
                .map(String::as_str)
                .unwrap_or("interrupt");
            if !matches!(delivery, "interrupt" | "queue" | "steer") {
                bail!("--delivery must be interrupt, queue, or steer");
            }
            let op = serde_json::from_value(
                json!({"type":"control","sessionId":pos[0],"action":{"type":"prompt","prompt":prompt,"delivery":delivery}}),
            )?;
            print_boss(boss_request(op)?)
        }
        "summon" => boss_summon(args),
        "file" | "persona" | "memory" | "plan" | "deliverable" | "speak" | "automation"
        | "resource-policy" | "open" | "browse" | "terminal" | "rename" | "avatar"
        | "report-blocker" | "employee" | "stop" | "resume" => boss_admin(action, args),
        _ => unreachable!(),
    }
}

fn boss_admin(group: &str, mut args: Vec<String>) -> anyhow::Result<()> {
    use waku_protocol::boss::BossOperation as Op;
    let operation = args.first().cloned().unwrap_or_default();
    if matches!(
        group,
        "rename"
            | "avatar"
            | "speak"
            | "report-blocker"
            | "open"
            | "browse"
            | "terminal"
            | "stop"
            | "resume"
    ) {
        args.insert(0, group.to_owned());
        return boss_admin_leaf(&args);
    }
    args.remove(0);
    match (group, operation.as_str()) {
        ("file", "list") => {
            let (pos, _) = flags(args, &[], true)?;
            if pos.len() > 1 {
                bail!("usage: boss file list [PATH]");
            }
            print_boss(boss_request(Op::ListFiles {
                path: pos.first().cloned().unwrap_or_default(),
            })?)
        }
        ("file", "read") => {
            let (pos, _) = flags(args, &[], true)?;
            if pos.len() != 1 {
                bail!("usage: boss file read PATH");
            }
            print_boss(boss_request(Op::ReadFile {
                path: pos[0].clone(),
            })?)
        }
        ("file", "write") => {
            let (pos, opts) = flags(args, &["file", "text"], true)?;
            if pos.len() != 1 {
                bail!("usage: boss file write PATH (--text TEXT|--file PATH|-)");
            }
            let content = raw_input(&opts)?;
            print_boss(boss_request(Op::WriteFile {
                path: pos[0].clone(),
                content,
            })?)
        }
        ("file", "mkdir") => {
            let (pos, _) = flags(args, &[], true)?;
            if pos.len() != 1 {
                bail!("usage: boss file mkdir PATH");
            }
            print_boss(boss_request(Op::CreateFolder {
                path: pos[0].clone(),
            })?)
        }
        ("persona", "list" | "show") => {
            let (pos, _) = flags(args, &[], true)?;
            let waku_protocol::boss::BossResult::State { state } = boss_request(Op::View)? else {
                bail!("unexpected Boss view result")
            };
            if operation == "list" && !pos.is_empty() {
                bail!("usage: boss persona list");
            }
            if operation == "show" && pos.len() != 1 {
                bail!("usage: boss persona show ID|NAME");
            }
            let selected: Vec<_> = if let Some(key) = pos.first() {
                if let Ok(id) = Uuid::parse_str(key) {
                    state.personas.iter().filter(|p| p.id == id).collect()
                } else {
                    state.personas.iter().filter(|p| p.name == *key).collect()
                }
            } else {
                state.personas.iter().collect()
            };
            if operation == "show" && selected.len() != 1 {
                bail!("persona is missing or ambiguous; use an exact name or UUID");
            }
            println!("{}", serde_json::to_string_pretty(&selected)?);
            Ok(())
        }
        ("persona", "upsert") => {
            let (_, opts) = flags(
                args,
                &["id", "name", "file", "text", "json", "json-file"],
                false,
            )?;
            let is_new = opts.contains_key("new");
            let id = opts.get("id");
            if is_new == id.is_some() {
                bail!("choose exactly one of --new or --id ID");
            }
            let persona_id = id
                .map(|id| id.parse().context("invalid persona ID"))
                .transpose()?
                .unwrap_or_else(Uuid::nil);
            let advanced = opts.contains_key("json") || opts.contains_key("json-file");
            let persona = if advanced {
                if opts.contains_key("file")
                    || opts.contains_key("text")
                    || opts.contains_key("name")
                {
                    bail!("--json/--json-file cannot be combined with --name, --text, or --file");
                }
                let input: PersonaUpsertInput = serde_json::from_str(&json_input(&opts)?)
                    .context("invalid persona configuration")?;
                waku_protocol::boss::BossPersonaUpsert {
                    id: persona_id,
                    name: input.name,
                    markdown: input.markdown,
                    pinned_files: input.pinned_files,
                    permissions: input.permissions,
                    icon: input.icon,
                }
            } else {
                let name = opts
                    .get("name")
                    .ok_or_else(|| anyhow!("persona upsert requires --name"))?;
                waku_protocol::boss::BossPersonaUpsert {
                    id: persona_id,
                    name: name.clone(),
                    markdown: raw_input(&opts)?,
                    pinned_files: Vec::new(),
                    permissions: Default::default(),
                    icon: None,
                }
            };
            print_boss(boss_request(Op::UpsertPersona { persona })?)
        }
        ("deliverable", "publish") => {
            let (pos, opts) = flags(args, &["name", "reference"], true)?;
            if pos.len() != 1 {
                bail!("usage: boss deliverable publish ABSOLUTE_PATH [--name NAME] [--reference]");
            }
            print_boss(boss_request(Op::PublishDeliverable {
                path: pos[0].clone(),
                name: opts.get("name").cloned(),
                reference: opts.contains_key("reference"),
            })?)
        }
        ("deliverable", "dismiss") => {
            let (pos, _) = flags(args, &[], true)?;
            if pos.len() != 1 {
                bail!("usage: boss deliverable dismiss ID");
            }
            print_boss(boss_request(Op::DismissDeliverable {
                id: pos[0].parse().context("invalid deliverable ID")?,
            })?)
        }
        ("memory", "buckets" | "create" | "overview" | "record" | "summary" | "scan" | "zoom") => {
            let memory = memory_leaf("boss memory", &operation, args)?;
            print_boss(boss_request(Op::Memory { operation: memory })?)
        }
        ("memory", "migrate") => {
            let (pos, options) = flags(args, &["dry-run"], true)?;
            if pos.len() != 2 {
                bail!("usage: boss memory migrate BUCKET boss|PROJECT_PATH [--dry-run true|false]");
            }
            let dry_run = options
                .get("dry-run")
                .map(|value| {
                    value
                        .parse::<bool>()
                        .context("--dry-run must be true or false")
                })
                .transpose()?
                .unwrap_or(true);
            let operation = waku_protocol::boss::MemoryOperation::MigrateLegacy {
                bucket: pos[0].clone(),
                source: pos[1].clone(),
                dry_run,
            };
            print_boss(boss_request(Op::Memory { operation })?)
        }
        ("plan", "finalize") => {
            let (pos, opts) = flags(args, &["items"], true)?;
            if pos.len() > 1 {
                bail!("usage: boss plan finalize [PLAN_FILE] [--items JSON]");
            }
            let items = opts
                .get("items")
                .map(|raw| {
                    serde_json::from_str::<Vec<String>>(raw)
                        .context("--items takes a JSON string array")
                })
                .transpose()?;
            print_boss(boss_request(Op::FinalizePlan {
                plan_file: pos.first().cloned(),
                items,
            })?)
        }
        ("plan", "items") => {
            let (pos, opts) = flags(args, &["json", "json-file"], true)?;
            if pos.len() != 1 {
                bail!("usage: boss plan items PLAN --json-file ITEMS.json");
            }
            let items: Vec<waku_protocol::boss::PlanItemInput> =
                serde_json::from_str(&json_input(&opts)?).context("invalid work item list")?;
            print_boss(boss_request(Op::UpdatePlanItems {
                plan: pos[0].clone(),
                items,
            })?)
        }
        ("plan", "item") => {
            let (pos, _) = flags(args, &[], true)?;
            if pos.len() != 3 {
                bail!("usage: boss plan item PLAN ITEM_ID toDo|done|dropped");
            }
            let op: Op = serde_json::from_value(json!({
                "type": "setPlanItemState",
                "plan": pos[0],
                "item": pos[1],
                "state": pos[2],
            }))
            .context("invalid setPlanItemState payload")?;
            print_boss(boss_request(op)?)
        }
        ("plan", "outcome") => {
            let (pos, _) = flags(args, &[], true)?;
            if pos.len() != 2 {
                bail!("usage: boss plan outcome PLAN completed|abandoned|approved");
            }
            let op: Op = serde_json::from_value(json!({
                "type": "setPlanOutcome",
                "plan": pos[0],
                "outcome": pos[1],
            }))
            .context("invalid setPlanOutcome payload")?;
            print_boss(boss_request(op)?)
        }
        ("plan", "create") => {
            let (_, opts) = flags(
                args,
                &[
                    "title",
                    "plan-file",
                    "file",
                    "text",
                    "provider",
                    "model",
                    "effort",
                ],
                false,
            )?;
            for key in ["title", "plan-file"] {
                if !opts.contains_key(key) {
                    bail!("boss plan create requires --{key}");
                }
            }
            let prompt = raw_input(&opts)?;
            let provider = opts.get("provider").map(|p| provider_kind(p)).transpose()?;
            print_boss(boss_request(Op::CreatePlan {
                title: opts["title"].clone(),
                plan_file: opts["plan-file"].clone(),
                prompt,
                provider,
                model: opts.get("model").cloned(),
                reasoning_effort: opts.get("effort").cloned(),
            })?)
        }
        ("employee", "rename") => {
            let (pos, _) = flags(args, &[], true)?;
            if pos.len() != 2 {
                bail!("usage: boss employee rename EMPLOYEE_ID NAME");
            }
            print_boss(boss_request(Op::RenameEmployee {
                session_id: pos[0].parse().context("invalid employee ID")?,
                name: pos[1].clone(),
            })?)
        }
        ("employee", "icon") => {
            let (pos, opts) = flags(args, &[], true)?;
            let clear = opts.contains_key("clear");
            if pos.len() != if clear { 1 } else { 2 } {
                bail!("usage: boss employee icon EMPLOYEE_ID ICON|--clear");
            }
            let icon = if clear {
                None
            } else {
                Some(serde_json::from_value(json!(pos[1])).context("invalid employee icon")?)
            };
            print_boss(boss_request(Op::SetEmployeeIcon {
                session_id: pos[0].parse().context("invalid employee ID")?,
                icon,
            })?)
        }
        ("employee", "plan") => {
            let (pos, opts) = flags(args, &["json", "json-file"], true)?;
            if pos.len() != 1 {
                bail!("usage: boss employee plan EMPLOYEE_ID --json JSON");
            }
            let id: Uuid = pos[0].parse().context("invalid employee ID")?;
            let input: serde_json::Value =
                serde_json::from_str(&json_input(&opts)?).context("invalid setPlan payload")?;
            let mut action = serde_json::Map::new();
            action.insert("type".into(), json!("setPlan"));
            for key in ["plan", "item"] {
                if let Some(value) = input.get(key) {
                    action.insert(key.into(), value.clone());
                }
            }
            let op: Op =
                serde_json::from_value(json!({"type":"control","sessionId":id,"action":action}))?;
            print_boss(boss_request(op)?)
        }
        ("employee", "permissions" | "workspace" | "resources") => {
            let (pos, opts) = flags(args, &["json", "json-file"], true)?;
            if pos.len() != 1 {
                bail!("usage: boss employee {operation} EMPLOYEE_ID --json-file CONFIG.json");
            }
            let id: Uuid = pos[0].parse().context("invalid employee ID")?;
            let input: serde_json::Value = serde_json::from_str(&json_input(&opts)?)
                .context("invalid employee configuration")?;
            let (action_type, field) = match operation.as_str() {
                "permissions" => ("setPermissions", "permissions"),
                "workspace" => ("setWorkspace", "workspace"),
                _ => ("setResources", "resources"),
            };
            let mut action = serde_json::Map::new();
            action.insert("type".into(), json!(action_type));
            action.insert(field.into(), input);
            let op: Op =
                serde_json::from_value(json!({"type":"control","sessionId":id,"action":action}))?;
            print_boss(boss_request(op)?)
        }
        ("employee", "model") => {
            let (pos, opts) = flags(args, &["provider", "model", "effort"], true)?;
            if pos.len() != 1 {
                bail!(
                    "usage: boss employee model EMPLOYEE_ID --provider PROVIDER --model MODEL [--effort ID]"
                );
            }
            let provider = opts
                .get("provider")
                .map(|p| provider_kind(p))
                .transpose()?
                .ok_or_else(|| anyhow!("--provider is required"))?;
            let model = opts
                .get("model")
                .cloned()
                .ok_or_else(|| anyhow!("--model is required"))?;
            let op: Op = serde_json::from_value(
                json!({"type":"control","sessionId":pos[0],"action":{"type":"setModel","provider":provider,"model":model,"reasoningEffort":opts.get("effort")}}),
            )?;
            print_boss(boss_request(op)?)
        }
        ("automation", "list") => print_boss(boss_request(Op::Automation {
            action: waku_protocol::boss::AutomationOperation::List,
        })?),
        ("automation", "create" | "update") => {
            let (_, opts) = flags(args, &["json", "json-file"], false)?;
            let input = json_input(&opts)?;
            let input = serde_json::from_str(&input).context("invalid automation configuration")?;
            let action = if operation == "create" {
                waku_protocol::boss::AutomationOperation::Create { input }
            } else {
                waku_protocol::boss::AutomationOperation::Update { input }
            };
            print_boss(boss_request(Op::Automation { action })?)
        }
        ("automation", "delete" | "pause" | "resume") => {
            let (pos, _) = flags(args, &[], true)?;
            if pos.len() != 1 {
                bail!("usage: boss automation {operation} AUTOMATION_ID");
            }
            let id = pos[0].parse().context("invalid automation ID")?;
            let action = match operation.as_str() {
                "delete" => waku_protocol::boss::AutomationOperation::Delete { automation_id: id },
                "pause" => waku_protocol::boss::AutomationOperation::Pause { automation_id: id },
                _ => waku_protocol::boss::AutomationOperation::Resume { automation_id: id },
            };
            print_boss(boss_request(Op::Automation { action })?)
        }
        ("resource-policy", "show") => print_boss(boss_request(Op::View)?),
        ("resource-policy", "set") => {
            let (_, opts) = flags(args, &["json", "json-file"], false)?;
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields, rename_all = "camelCase")]
            struct PolicyInput {
                expected_revision: u64,
                model_limits: Vec<waku_protocol::boss::ModelLimit>,
                #[serde(default)]
                host: Option<waku_protocol::resources::ResourcePolicy>,
            }
            let input: PolicyInput = serde_json::from_str(&json_input(&opts)?)
                .context("invalid resource policy configuration")?;
            print_boss(boss_request(Op::SetResourcePolicy {
                expected_revision: input.expected_revision,
                model_limits: input.model_limits,
                host: input.host,
            })?)
        }
        _ => {
            let mut leaf = vec![group.to_owned(), operation];
            leaf.extend(args);
            boss_admin_leaf(&leaf)
        }
    }
}

/// Parse one `memory` operation's flags into its wire shape. `command` is
/// the usage-string label — `boss memory` for the boss surface, `memory`
/// for the session-scoped top-level command.
fn memory_leaf(
    command: &str,
    operation: &str,
    args: Vec<String>,
) -> anyhow::Result<waku_protocol::boss::MemoryOperation> {
    let kind = match operation {
        "buckets" => "listBuckets",
        "create" => "createBucket",
        "record" => "record",
        "summary" => "submitSummary",
        "scan" => "scan",
        "zoom" => "zoomBucket",
        other => other,
    };
    let memory = if matches!(operation, "create" | "record" | "summary") {
        let (_, opts) = flags(args, &["json", "json-file", "project"], false)?;
        let mut value: serde_json::Value =
            serde_json::from_str(&json_input(&opts)?).context("invalid memory input")?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| anyhow!("memory input must be a JSON object"))?;
        object.insert("type".into(), json!(kind));
        if let Some(project) = opts.get("project") {
            if operation == "create" {
                bail!("--project is not valid for {command} create");
            }
            if object.contains_key("project") {
                bail!("--project duplicates the payload's project field");
            }
            object.insert("project".into(), json!(project));
        }
        serde_json::from_value(value)?
    } else {
        let (pos, opts) = flags(args, &["project"], true)?;
        let project = opts.get("project");
        let value = match operation {
            "buckets" if pos.is_empty() && project.is_none() => json!({"type":kind}),
            "overview" => match (pos.len(), project) {
                (0, Some(project)) => json!({"type":kind,"project":project}),
                (0, None) => json!({"type":kind}),
                (1, None) => json!({"type":kind,"bucket":pos[0]}),
                _ => bail!("usage: {command} overview [BUCKET|--project PROJECT]"),
            },
            "scan" => match (pos.len(), project) {
                (1, Some(project)) => {
                    json!({"type":kind,"project":project,"query":pos[0]})
                }
                (1, None) => json!({"type":kind,"query":pos[0]}),
                (2, None) => json!({"type":kind,"bucket":pos[0],"query":pos[1]}),
                _ => bail!("usage: {command} scan [BUCKET|--project PROJECT] QUERY"),
            },
            "zoom" => match (pos.len(), project) {
                (2, Some(project)) => {
                    json!({"type":kind,"project":project,"start":pos[0].parse::<u64>().context("invalid start note index")?,"end":pos[1].parse::<u64>().context("invalid end note index")?})
                }
                (2, None) => {
                    json!({"type":kind,"start":pos[0].parse::<u64>().context("invalid start note index")?,"end":pos[1].parse::<u64>().context("invalid end note index")?})
                }
                (3, None) => {
                    json!({"type":kind,"bucket":pos[0],"start":pos[1].parse::<u64>().context("invalid start note index")?,"end":pos[2].parse::<u64>().context("invalid end note index")?})
                }
                _ => bail!("usage: {command} zoom [BUCKET|--project PROJECT] START END"),
            },
            _ => bail!("usage: {command} {operation} [BUCKET|--project PROJECT] [QUERY|START END]"),
        };
        serde_json::from_value(value)?
    };
    Ok(memory)
}

fn boss_admin_leaf(args: &[String]) -> anyhow::Result<()> {
    use waku_protocol::boss::BossOperation as Op;
    match args.first().map(String::as_str).unwrap_or("") {
        "stop" => {
            if args.len() != 2 {
                bail!("usage: boss stop EMPLOYEE_ID");
            }
            let operation = serde_json::from_value(
                json!({"type":"control","sessionId":args[1],"action":{"type":"stop"}}),
            )?;
            print_boss(boss_request(operation)?)
        }
        "resume" => {
            if args.len() != 2 {
                bail!("usage: boss resume EMPLOYEE_ID");
            }
            let session_id = args[1].parse().context("invalid employee ID")?;
            print_boss(boss_request(Op::Resume { session_id })?)
        }
        "rename" => {
            if args.len() != 2 {
                bail!("usage: boss rename NAME");
            }
            print_boss(boss_request(Op::Rename {
                name: args[1].clone(),
            })?)
        }
        "report-blocker" => {
            let (_, opts) = flags(args[1..].to_vec(), &["text", "file"], false)?;
            let message = raw_input(&opts)?;
            print_boss(boss_request(Op::ReportBlocker { message })?)
        }
        "speak" => {
            let (_, opts) = flags(args[1..].to_vec(), &["text", "file"], false)?;
            print_boss(boss_request(Op::Speak {
                parts: vec![raw_input(&opts)?],
            })?)
        }
        "browse" => {
            if args.len() != 2 {
                bail!("usage: boss browse URL");
            }
            print_boss(boss_request(Op::Browse {
                url: args[1].clone(),
                title: None,
            })?)
        }
        "avatar" => {
            if args.first().map(String::as_str) != Some("regenerate") || args.len() > 2 {
                bail!("usage: boss avatar regenerate [EMPLOYEE_ID]");
            }
            let session_id = args
                .get(1)
                .map(|id| id.parse().context("invalid employee ID"))
                .transpose()?;
            print_boss(boss_request(Op::RegenerateAvatar { session_id })?)
        }
        "open" => {
            let (_, opts) = flags(args[1..].to_vec(), &["provider", "model", "mode"], false)?;
            let provider = opts
                .get("provider")
                .map(|p| provider_kind(p))
                .transpose()?
                .ok_or_else(|| anyhow!("boss open requires --provider"))?;
            let mode: waku_protocol::model::RuntimeMode = serde_json::from_value(json!(
                opts.get("mode")
                    .map(String::as_str)
                    .unwrap_or("autoAcceptEdits")
            ))
            .context("invalid runtime mode")?;
            print_boss(boss_request(Op::Open {
                provider,
                model: opts.get("model").cloned(),
                mode,
            })?)
        }
        "terminal" => {
            let (_, opts) = flags(args[1..].to_vec(), &["title", "cwd", "file"], false)?;
            let title = opts
                .get("title")
                .cloned()
                .ok_or_else(|| anyhow!("boss terminal requires --title"))?;
            let cwd = opts
                .get("cwd")
                .cloned()
                .unwrap_or(std::env::current_dir()?.display().to_string());
            let command = opts
                .get("file")
                .map(|p| std::fs::read_to_string(p).with_context(|| format!("reading {p}")))
                .transpose()?;
            print_boss(boss_request(Op::Terminal {
                title,
                cwd,
                command,
            })?)
        }
        _ => bail!("unsupported Boss command path: {}", args.join(" ")),
    }
}

fn json_input(opts: &std::collections::BTreeMap<String, String>) -> anyhow::Result<String> {
    let json = opts.get("json");
    let file = opts.get("json-file");
    if json.is_some() == file.is_some() {
        bail!("provide exactly one of --json OBJECT or --json-file PATH|-");
    }
    if let Some(value) = json {
        return Ok(value.clone());
    }
    let path = file.unwrap();
    if path != "-" {
        return std::fs::read_to_string(path).with_context(|| format!("reading {path}"));
    }
    let mut body = String::new();
    std::io::Read::read_to_string(
        &mut std::io::Read::take(std::io::stdin().lock(), 4 * 1024 * 1024 + 1),
        &mut body,
    )?;
    if body.len() > 4 * 1024 * 1024 {
        bail!("JSON input exceeds 4 MB");
    }
    Ok(body)
}

fn boss_summon(args: Vec<String>) -> anyhow::Result<()> {
    use waku_protocol::boss::BossOperation;
    let (_, opts) = flags(
        args,
        &[
            "persona",
            "title",
            "icon",
            "project",
            "provider",
            "model",
            "effort",
            "work-goal",
            "plan",
            "item",
            "workspace",
            "base-branch",
            "adopt-worktree",
            "request-id",
            "file",
            "text",
        ],
        false,
    )?;
    for required in ["persona", "title"] {
        if !opts.contains_key(required) {
            bail!("boss summon requires --{required}");
        }
    }
    let prompt = raw_input(&opts)?;
    if prompt.trim().is_empty() {
        bail!("prompt must not be empty");
    }
    let persona_key = &opts["persona"];
    let state = match boss_request(BossOperation::View)? {
        waku_protocol::boss::BossResult::State { state } => state,
        other => bail!("unexpected Boss view result: {other:?}"),
    };
    let persona = if let Ok(id) = Uuid::parse_str(persona_key) {
        state.personas.iter().find(|p| p.id == id)
    } else {
        let matches: Vec<_> = state
            .personas
            .iter()
            .filter(|p| p.name == *persona_key)
            .collect();
        if matches.len() > 1 {
            bail!(
                "persona name is ambiguous; use an ID: {}",
                matches
                    .iter()
                    .map(|p| p.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        matches.first().copied()
    }
    .ok_or_else(|| anyhow!("unknown persona `{persona_key}`"))?;
    let workspace = opts.get("workspace").map(String::as_str).unwrap_or("local");
    if !matches!(workspace, "local" | "worktree" | "adopt") {
        bail!("--workspace must be local, worktree, or adopt");
    }
    if workspace == "worktree" && !opts.contains_key("base-branch") {
        bail!("--base-branch is required with --workspace worktree");
    }
    if workspace == "adopt" && !opts.contains_key("adopt-worktree") {
        bail!("--adopt-worktree is required with --workspace adopt");
    }
    let project = opts
        .get("project")
        .cloned()
        .unwrap_or(std::env::current_dir()?.display().to_string());
    let icon: Option<CustomCommandIcon> = opts
        .get("icon")
        .map(|i| serde_json::from_value(json!(i)).context("invalid employee icon"))
        .transpose()?;
    let operation: BossOperation = serde_json::from_value(json!({
        "type":"summon", "personaId":persona.id, "jobTitle":opts["title"], "prompt":prompt,
        "project":project, "provider":opts.get("provider"), "model":opts.get("model"),
        "reasoningEffort":opts.get("effort"), "workspace":workspace, "baseBranch":opts.get("base-branch"),
        "adoptWorktree":opts.get("adopt-worktree"), "workGoal":opts.get("work-goal").map(String::as_str).unwrap_or("errand"),
        "icon":icon, "plan":opts.get("plan"), "item":opts.get("item"), "requestId":opts.get("request-id")
    }))?;
    match boss_request(operation)? {
        waku_protocol::boss::BossResult::Summoned {
            session_id,
            state,
            admission,
        } => {
            let admission_json = admission.as_ref().map(|a| json!({"provider":a.provider,"model":a.model,"reasoningEffort":a.reasoning_effort,"queuePosition":a.queue_position,"blockedBy":a.blocked_by}));
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "name":persona.name,"id":session_id,"provider":admission.as_ref().map(|a|a.provider),
                    "model":admission.as_ref().map(|a|a.model.as_str()),"effort":admission.as_ref().and_then(|a|a.reasoning_effort.as_deref()),
                    "workGoal":opts.get("work-goal").map(String::as_str).unwrap_or("errand"),
                    "project":project,"workspace":workspace,"state":state,"admission":admission_json
                }))?
            );
            Ok(())
        }
        other => print_boss(other),
    }
}

fn task_everyday(action: &str, args: Vec<String>) -> anyhow::Result<()> {
    let command = if action == "prompt" {
        let (pos, opts) = flags(args, &["file", "text", "delivery"], true)?;
        if pos.len() != 1 {
            bail!("usage: goddard-agent prompt TASK_ID (--text TEXT|--file PATH|-)");
        }
        let prompt = raw_input(&opts)?;
        if prompt.trim().is_empty() {
            bail!("prompt must not be empty");
        }
        let delivery = opts
            .get("delivery")
            .map(String::as_str)
            .unwrap_or("interrupt");
        let delivery = match delivery {
            "interrupt" => AgentPromptDelivery::Interrupt,
            "queue" => AgentPromptDelivery::Queue,
            "steer" => AgentPromptDelivery::Steer,
            _ => bail!("--delivery must be interrupt, queue, or steer"),
        };
        Command::AgentPrompt {
            task_id: Some(pos[0].parse().context("invalid task ID")?),
            thread_id: None,
            provider: None,
            prompt,
            delivery,
        }
    } else {
        let (pos, opts) = flags(args, &["turn"], true)?;
        if pos.len() > 1 {
            bail!("usage: goddard-agent read [TASK_ID] [--turn N]");
        }
        Command::AgentReadSession {
            task_id: pos
                .first()
                .map(|id| id.parse().context("invalid task ID"))
                .transpose()?,
            thread_id: None,
            provider: None,
            turn: opts
                .get("turn")
                .map(|v| v.parse().context("--turn must be a positive integer"))
                .transpose()?,
        }
    };
    let response = connect()?.request(request_session_id(), Uuid::nil(), command)?;
    match response {
        ResponsePayload::AgentSessionTranscript { transcript } => {
            println!("{}", serde_json::to_string_pretty(&transcript)?)
        }
        ResponsePayload::Ack => println!("{{\"ok\":true}}"),
        other => bail!("unexpected response: {other:?}"),
    }
    Ok(())
}

fn task_create(args: Vec<String>) -> anyhow::Result<()> {
    let (_, opts) = flags(
        args,
        &[
            "project",
            "workspace",
            "base-branch",
            "provider",
            "model",
            "title",
            "effort",
            "service-tier",
            "context-window",
            "text",
            "file",
        ],
        false,
    )?;
    let project = opts
        .get("project")
        .ok_or_else(|| anyhow!("create requires --project PATH"))?;
    let prompt = raw_input(&opts)?;
    if prompt.trim().is_empty() {
        bail!("prompt must not be empty");
    }
    let workspace = opts.get("workspace").map(String::as_str).unwrap_or("local");
    let workspace: AgentWorkspace = match workspace {
        "local" => AgentWorkspace::Local,
        "worktree" => AgentWorkspace::Worktree,
        _ => bail!("--workspace must be local or worktree"),
    };
    let base_branch = opts.get("base-branch").cloned();
    if workspace == AgentWorkspace::Worktree && base_branch.is_none() {
        bail!("--base-branch is required with --workspace worktree");
    }
    let command = Command::AgentCreateSession {
        provider: opts.get("provider").map(|p| provider_kind(p)).transpose()?,
        model: opts.get("model").cloned(),
        project: project.into(),
        workspace,
        base_branch,
        prompt,
        title: opts.get("title").cloned(),
        reasoning_effort: opts.get("effort").cloned(),
        service_tier: opts.get("service-tier").cloned(),
        context_window: opts.get("context-window").cloned(),
    };
    match connect()?.request(request_session_id(), Uuid::nil(), command)? {
        ResponsePayload::AgentSessionCreated { session_id } => {
            println!("{}", json!({"task_id":session_id}));
            Ok(())
        }
        other => bail!("unexpected response: {other:?}"),
    }
}

fn task_map(args: Vec<String>) -> anyhow::Result<()> {
    let (_, opts) = flags(
        args,
        &[
            "text",
            "file",
            "path",
            "intent",
            "max-tokens",
            "anchors",
            "known-paths",
        ],
        false,
    )?;
    let query = raw_input(&opts)?;
    let intent: ProjectMapIntent = opts
        .get("intent")
        .map(|v| {
            serde_json::from_value(json!(v))
                .context("--intent must be locate, understand, or change")
        })
        .transpose()?
        .unwrap_or_default();
    let anchors = opts
        .get("anchors")
        .map(|v| serde_json::from_str(v).context("--anchors must be a JSON string array"))
        .transpose()?
        .unwrap_or_default();
    let known_paths = opts
        .get("known-paths")
        .map(|v| serde_json::from_str(v).context("--known-paths must be a JSON path array"))
        .transpose()?
        .unwrap_or_default();
    let command = Command::AgentProjectMap {
        query,
        path: opts.get("path").map(PathBuf::from),
        max_tokens: opts
            .get("max-tokens")
            .map(|v| v.parse().context("--max-tokens must be an integer"))
            .transpose()?,
        intent,
        anchors,
        known_paths,
    };
    match connect()?.request(request_session_id(), Uuid::nil(), command)? {
        ResponsePayload::AgentProjectMap { result } => {
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        other => bail!("unexpected response: {other:?}"),
    }
}

fn task_search(args: Vec<String>) -> anyhow::Result<()> {
    let (_, opts) = flags(args, &["text", "file", "last-turns"], false)?;
    let query = raw_input(&opts)?;
    if query.trim().is_empty() {
        bail!("search query must not be empty");
    }
    let last_turns = opts
        .get("last-turns")
        .map(|v| v.parse().context("--last-turns must be a positive integer"))
        .transpose()?;
    match connect()?.request(
        request_session_id(),
        Uuid::nil(),
        Command::AgentSearchSessions { query, last_turns },
    )? {
        ResponsePayload::AgentSessionSearch { hits } => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"results":hits,"session_link_hint":session_link_hint()})
                )?
            );
            Ok(())
        }
        other => bail!("unexpected response: {other:?}"),
    }
}

/// `goddard-agent memory` — the session-scoped bucket surface for ordinary
/// task agents and employees. Unqualified operations land on the session's
/// own project bucket; `--project`/`BUCKET` address registered projects or
/// granted buckets. Bucket creation and legacy migration stay Boss-only and
/// are reached through `boss memory`.
fn task_memory(args: Vec<String>) -> anyhow::Result<()> {
    use waku_protocol::boss::BossOperation;
    let operation = args.first().cloned().unwrap_or_default();
    match operation.as_str() {
        "buckets" | "overview" | "record" | "summary" | "scan" | "zoom" => {
            let memory = memory_leaf("memory", &operation, args[1..].to_vec())?;
            print_boss(boss_request(BossOperation::Memory { operation: memory })?)
        }
        "create" | "migrate" => bail!("`memory {operation}` is a Boss-only operation"),
        _ => bail!(
            "usage: goddard-agent memory buckets|overview|record|summary|scan|zoom; each leaf documents its flags under --help"
        ),
    }
}

fn boss_request(
    operation: waku_protocol::boss::BossOperation,
) -> anyhow::Result<waku_protocol::boss::BossResult> {
    match connect()?.request(
        request_session_id(),
        Uuid::nil(),
        Command::Boss { operation },
    )? {
        ResponsePayload::Boss { result } => Ok(result),
        other => bail!("unexpected Boss response: {other:?}"),
    }
}

fn print_boss(result: waku_protocol::boss::BossResult) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn computer_command(arguments: &[String]) -> anyhow::Result<Command> {
    let command = match arguments {
        [action] if action == "reset" => Command::AgentComputerUseReset,
        [action, payload] if action == "js" => {
            let payload = computer_payload(payload, 1024 * 1024)?;
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                code: String,
                #[serde(default)]
                timeout_ms: Option<u64>,
                #[serde(default)]
                title: Option<String>,
            }
            let input: Input =
                serde_json::from_str(&payload).context("invalid computer js payload")?;
            Command::AgentComputerUse {
                code: input.code,
                timeout_ms: input.timeout_ms,
                title: input.title,
            }
        }
        [action, payload] if action == "run" => {
            let payload = computer_payload(payload, 64 * 1024)?;
            let request: ComputerUseRunRequest =
                serde_json::from_str(&payload).context("invalid computer run payload")?;
            Command::AgentComputerUseRun { request }
        }
        _ => bail!("use `computer js '<json>'`, `computer run '<json>'`, or `computer reset`"),
    };
    Ok(command)
}

fn computer_payload(payload: &str, max_bytes: usize) -> anyhow::Result<String> {
    if payload != "--stdin" {
        if payload.len() > max_bytes {
            bail!("computer payload exceeds {} bytes", max_bytes);
        }
        return Ok(payload.to_owned());
    }
    let mut input = String::new();
    std::io::Read::read_to_string(
        &mut std::io::Read::take(std::io::stdin().lock(), max_bytes as u64 + 1),
        &mut input,
    )?;
    if input.len() > max_bytes {
        bail!("computer payload exceeds {} bytes", max_bytes);
    }
    Ok(input)
}

fn computer(arguments: Vec<String>) -> anyhow::Result<()> {
    let command = computer_command(&arguments)?;
    match connect()?.request_with_timeout(request_session_id(), Uuid::nil(), command, None)? {
        ResponsePayload::AgentComputerUseResult { result } => {
            println!("{}", serde_json::to_string_pretty(&result)?);
            if result["isError"] == true {
                bail!("computer-use JavaScript failed; see the result above");
            }
            Ok(())
        }
        other => bail!("daemon returned an unexpected response: {other:?}"),
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
        "boss" => Ok(Command::Boss {
            operation: parse_boss_operation(payload)?,
        }),
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
                title: payload.title,
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
                    DeliveryArg::Interrupt => AgentPromptDelivery::Interrupt,
                    DeliveryArg::Queue => AgentPromptDelivery::Queue,
                    DeliveryArg::Steer => AgentPromptDelivery::Steer,
                },
            })
        }
        "rename" => {
            let payload: RenamePayload = serde_json::from_str(payload)
                .context("`rename` takes a JSON object with a title")?;
            Ok(Command::AgentRenameSelf {
                title: payload.title,
            })
        }
        "archive" => {
            let payload: ArchivePayload = serde_json::from_str(payload)
                .context("`archive` takes a JSON object with task_ids")?;
            Ok(Command::AgentProposeArchive {
                task_ids: payload.task_ids,
                reason: payload.reason,
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
        "search" => {
            let payload: SearchPayload = serde_json::from_str(payload).context(
                "`search` takes a JSON object; run `goddard-agent schema` for its shape",
            )?;
            Ok(Command::AgentSearchSessions {
                query: payload.query,
                last_turns: payload.last_turns,
            })
        }
        "map" => {
            let payload: ProjectMapPayload = serde_json::from_str(payload)
                .context("`map` takes a JSON object; run `goddard-agent schema` for its shape")?;
            Ok(Command::AgentProjectMap {
                query: payload.query,
                path: payload.path,
                max_tokens: payload.max_tokens,
                intent: payload.intent,
                anchors: payload.anchors,
                known_paths: payload.known_paths,
            })
        }
        "ask" => Ok(Command::AgentAsk {
            questions: ask_questions(payload)?,
        }),
        _ => unreachable!("checked by run()"),
    }
}

fn parse_boss_operation(payload: &str) -> anyhow::Result<waku_protocol::boss::BossOperation> {
    #[derive(Deserialize)]
    struct Header {
        #[serde(rename = "type")]
        operation: String,
    }
    let header: Header = serde_json::from_str(payload).context("invalid Boss operation JSON")?;
    let operation = header.operation.as_str();
    if matches!(
        operation,
        "markDeliverableViewed"
            | "markGoalsViewed"
            | "pinDeliverable"
            | "sweepDeliverable"
            | "archiveDeliverable"
    ) {
        bail!(
            "{operation} is client-owned sidebar state and is not available through the agent CLI"
        );
    }
    if matches!(operation, "view" | "roster" | "context") {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
        enum NoInputBossOperation {
            View,
            Roster,
            Context,
        }
        let _: NoInputBossOperation =
            serde_json::from_str(payload).context("no-input Boss operation accepts no fields")?;
    }
    serde_json::from_str(payload)
        .context("`boss` takes a typed JSON operation; run `goddard-agent schema`")
}

/// One line appended to `search` output so the agent knows how to turn a
/// hit into a transcript link — the app renders `[title](goddard://task/<id>)`
/// as a link that opens that task, and `?message=<message_id>` scrolls it to
/// the matched message.
fn session_link_hint() -> String {
    format!(
        "Reference a task in your reply as [title]({}<task_id>) and Goddard renders it as a link that opens the task. With a search hit's messageId, [title]({}<task_id>?message=<message_id>) lands on the matched message.",
        waku_protocol::TASK_LINK_PREFIX,
        waku_protocol::TASK_LINK_PREFIX
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct AskOption {
    label: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct AskQuestion {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    header: Option<String>,
    question: String,
    #[serde(default)]
    options: Vec<AskOption>,
    #[serde(default)]
    multi_select: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskPayload {
    questions: Vec<AskQuestion>,
}

/// Parse the `ask` payload into the wire question shape — ids and headers
/// default like the provider drivers' own translation layers.
fn ask_questions(payload: &str) -> anyhow::Result<Vec<UserInputQuestion>> {
    let payload: AskPayload = serde_json::from_str(payload)
        .context("`ask` takes a JSON object; run `goddard-agent schema` for its shape")?;
    if payload.questions.is_empty() {
        bail!("`ask` requires at least one question");
    }
    payload
        .questions
        .into_iter()
        .enumerate()
        .map(|(index, question)| {
            if question.question.trim().is_empty() {
                bail!("question {index} has no text");
            }
            Ok(UserInputQuestion {
                id: question
                    .id
                    .filter(|id| !id.trim().is_empty())
                    .unwrap_or_else(|| format!("question-{index}")),
                header: question
                    .header
                    .filter(|header| !header.trim().is_empty())
                    .unwrap_or_else(|| "Question".to_owned()),
                question: question.question,
                options: question
                    .options
                    .into_iter()
                    .filter(|option| !option.label.trim().is_empty())
                    .map(|option| UserInputOption {
                        label: option.label,
                        description: option
                            .description
                            .filter(|description| !description.trim().is_empty()),
                    })
                    .collect(),
                multi_select: question.multi_select,
            })
        })
        .collect()
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
    #[test]
    fn computer_commands_accept_js_and_bounded_run_payloads_without_task_selectors() {
        let command = super::computer_command(&[
            "js".into(),
            r#"{"code":"var value = 42","timeout_ms":1000,"title":"Initialize"}"#.into(),
        ])
        .unwrap();
        assert!(matches!(
            command,
            Command::AgentComputerUse {
                timeout_ms: Some(1000),
                ..
            }
        ));
        assert!(matches!(
            super::computer_command(&["reset".into()]).unwrap(),
            Command::AgentComputerUseReset
        ));
        assert!(super::computer_command(&["reset".into(), "{}".into()]).is_err());
        assert!(
            super::computer_command(&["js".into(), r#"{"code":"1","task_id":"foreign"}"#.into()])
                .is_err()
        );
        let run = super::computer_command(&[
            "run".into(),
            r#"{"url":"https://example.test/form","goal":"Fill the form","values":{"Email":"user@example.test"},"verify":{"textContains":["Saved"]}}"#.into(),
        ])
        .unwrap();
        assert!(matches!(
            run,
            Command::AgentComputerUseRun { request }
                if request.url == "https://example.test/form"
                    && request.values.get("Email").is_some_and(|value| value == "user@example.test")
        ));
        assert!(
            super::computer_command(&[
                "run".into(),
                r#"{"url":"https://example.test","goal":"x","task_id":"foreign"}"#.into()
            ])
            .is_err()
        );
    }

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
                ..
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
    fn a_prompt_payload_defaults_to_interrupt_delivery() {
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
                assert_eq!(delivery, AgentPromptDelivery::Interrupt);
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
    fn a_search_payload_becomes_an_agent_search_command() {
        let command = build_command("search", r#"{"query":"status:idle retry logic"}"#)
            .expect("a query payload parses");

        match command {
            Command::AgentSearchSessions { query, last_turns } => {
                assert_eq!(query, "status:idle retry logic");
                assert_eq!(last_turns, None);
            }
            other => panic!("expected AgentSearchSessions, got {other:?}"),
        }
        assert!(build_command("search", "{}").is_err());
    }

    #[test]
    fn a_map_payload_carries_the_agent_retrieval_hints() {
        let command = build_command(
            "map",
            r#"{"query":"What controls expiry?","path":"src/auth","intent":"change","anchors":["Session::refresh"],"known_paths":["src/auth/session.rs"],"max_tokens":1800}"#,
        )
        .expect("a valid map request parses");
        match command {
            Command::AgentProjectMap {
                query,
                path,
                max_tokens,
                intent,
                anchors,
                known_paths,
            } => {
                assert_eq!(query, "What controls expiry?");
                assert_eq!(path.as_deref(), Some(std::path::Path::new("src/auth")));
                assert_eq!(max_tokens, Some(1800));
                assert_eq!(intent, ProjectMapIntent::Change);
                assert_eq!(anchors, ["Session::refresh"]);
                assert_eq!(known_paths, [PathBuf::from("src/auth/session.rs")]);
            }
            other => panic!("expected AgentProjectMap, got {other:?}"),
        }
        assert!(build_command("map", "{}").is_err());
        assert!(build_command("map", r#"{"query":"x","intent":"scope"}"#).is_err());
    }

    #[test]
    fn a_search_payload_may_confine_the_scan_to_the_last_turns() {
        let command = build_command("search", r#"{"query":"retry logic","last_turns":2}"#)
            .expect("a last_turns payload parses");

        match command {
            Command::AgentSearchSessions { query, last_turns } => {
                assert_eq!(query, "retry logic");
                assert_eq!(last_turns, Some(2));
            }
            other => panic!("expected AgentSearchSessions, got {other:?}"),
        }
    }

    #[test]
    fn rename_payload_only_carries_a_title() {
        let command = build_command("rename", r#"{"title":"My task"}"#).unwrap();
        assert!(matches!(command, Command::AgentRenameSelf { title } if title == "My task"));
        assert!(build_command("rename", "{}").is_err());
    }

    #[test]
    fn an_archive_payload_carries_task_ids_and_an_optional_reason() {
        let id = Uuid::new_v4();
        let command = build_command(
            "archive",
            &format!(r#"{{"task_ids":["{id}"],"reason":"work landed"}}"#),
        )
        .unwrap();
        assert!(matches!(
            command,
            Command::AgentProposeArchive { task_ids, reason }
                if task_ids == [id] && reason.as_deref() == Some("work landed")
        ));
        let command = build_command("archive", &format!(r#"{{"task_ids":["{id}"]}}"#)).unwrap();
        assert!(matches!(
            command,
            Command::AgentProposeArchive { task_ids, reason: None } if task_ids == [id]
        ));
        assert!(build_command("archive", "{}").is_err());
        assert!(build_command("archive", r#"{"task_ids":"not-a-list"}"#).is_err());
    }

    #[test]
    fn the_link_hint_names_the_task_link_format() {
        let hint = session_link_hint();
        assert!(hint.contains(waku_protocol::TASK_LINK_PREFIX));
        assert!(hint.contains("messageId"));
        assert!(hint.contains("?message="));
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
    fn an_ask_payload_becomes_an_agent_ask_command_with_defaults() {
        let command = build_command(
            "ask",
            r#"{"questions":[{"question":"Which environment?","options":[{"label":"Staging"},{"label":"Production","description":"Requires sign-off"}],"multiSelect":false},{"id":"confirm","header":"Deploy","question":"Ship it?"}]}"#,
        )
        .expect("a valid ask payload parses");

        match command {
            Command::AgentAsk { questions } => {
                assert_eq!(questions.len(), 2);
                assert_eq!(questions[0].id, "question-0");
                assert_eq!(questions[0].header, "Question");
                assert_eq!(questions[0].options.len(), 2);
                assert_eq!(
                    questions[0].options[1].description.as_deref(),
                    Some("Requires sign-off")
                );
                assert!(!questions[0].multi_select);
                assert_eq!(questions[1].id, "confirm");
                assert_eq!(questions[1].header, "Deploy");
                assert!(questions[1].options.is_empty());
            }
            other => panic!("expected AgentAsk, got {other:?}"),
        }
    }

    #[test]
    fn an_ask_payload_rejects_empty_and_blank_questions() {
        assert!(build_command("ask", r#"{"questions":[]}"#).is_err());
        assert!(build_command("ask", r#"{"questions":[{"question":"  "}]}"#).is_err());
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
        assert!(USAGE.contains("goddard-agent map"));
        assert!(schema.contains("\"--anchors\""));
        let blocker = leaf_schema("boss report-blocker");
        let description = blocker["inputs"]["description"].as_str().unwrap();
        assert!(!description.is_empty());
        assert!(blocker["help"].as_str().unwrap().contains(description));
        // The schema stays machine-readable.
        let _: serde_json::Value = serde_json::from_str(&schema).unwrap();
    }

    #[test]
    fn everyday_flags_enforce_exact_inputs_and_preserve_text() {
        let (_, options) = flags(
            vec!["--text".into(), "line one\n$literal `tick`\n".into()],
            &["text", "file"],
            false,
        )
        .unwrap();
        assert_eq!(raw_input(&options).unwrap(), "line one\n$literal `tick`\n");
        assert!(raw_input(&std::collections::BTreeMap::new()).is_err());
        let (_, both) = flags(
            vec!["--text".into(), "a".into(), "--file".into(), "b".into()],
            &["text", "file"],
            false,
        )
        .unwrap();
        assert!(raw_input(&both).is_err());
        assert!(flags(vec!["--misspelled".into()], &["text"], false).is_err());
        assert!(
            flags(
                vec!["--text".into(), "a".into(), "--text".into(), "b".into()],
                &["text"],
                false
            )
            .is_err()
        );
        assert!(
            build_command(
                "create",
                r#"{"project":"/tmp","workspace":"local","prompt":"x","misspelled":true}"#
            )
            .is_err()
        );
        assert!(
            build_command(
                "create",
                r#"{"project":"/tmp","workspace":"local","prompt":"x","prompt":"y"}"#
            )
            .is_err()
        );
        assert!(build_command("boss", r#"{"type":"pinDeliverable","id":"00000000-0000-0000-0000-000000000001","pinned":true}"#).is_err());
        let exported = schema();
        assert!(exported["commands"]["boss summon"].is_object());
        assert!(exported["commands"]["map"]["inputs"]["--anchors"].is_object());
        assert!(
            exported.get("boss").is_none(),
            "the old Boss union is not in discovery"
        );
    }

    #[test]
    fn every_advertised_leaf_has_specific_inputs_and_outputs() {
        let commands = schema()["commands"].as_object().unwrap().clone();
        assert!(!commands.contains_key("boss file"));
        assert!(!commands.contains_key("boss employee stop"));
        for (path, leaf) in &commands {
            assert_eq!(
                leaf["command"].as_str(),
                Some(path.as_str()),
                "schema command path mismatch"
            );
            assert!(
                leaf["inputs"].is_object(),
                "{path} must describe its inputs"
            );
            assert!(
                leaf["outputs"].is_object(),
                "{path} must describe its output"
            );
            assert!(leaf["example"].is_string(), "{path} must have an example");
            assert!(
                leaf["inputs"].get("input").is_none(),
                "{path} must not use the old generic input placeholder"
            );
        }
    }
}
