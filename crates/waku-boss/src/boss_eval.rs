//! The boss's Rhai script loop: one stateless invocation batches
//! `BossOperation`s whose results feed later calls in that script.
//!
//! Rhai functions must be `'static`, so the script cannot borrow the
//! dispatching `&self` it needs. Instead the script runs on its own thread
//! and the bound functions send each operation over a channel; the calling
//! thread runs it through the normal `handle_boss_operation` path — keeping
//! every operation's own authorization — and replies with the serialized
//! result. The channel doubles as the timeout's lever: an abandoned script
//! fails its next bound call the moment the receiver drops, and the
//! engine's progress callback terminates it even mid-computation.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender, SyncSender, sync_channel};
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use parking_lot::Mutex;
use rhai::{Array, Dynamic, Engine, EvalAltResult, ImmutableString, Map, Position, Scope};
use waku_protocol::boss::{BossOperation, BossResult};

/// Longest script source one `script` invocation accepts.
pub const MAX_EVAL_SCRIPT_BYTES: usize = 256 * 1024;
/// Wall-clock budget for one `eval` — script time plus every operation it
/// dispatches. A script inside a bound operation when the deadline lands
/// survives until that operation returns or the dispatcher is dropped.
const EVAL_WALL_CLOCK: Duration = Duration::from_secs(30);
/// Grace for a cancelled script to unwind through rhai's progress callback
/// before its worker is abandoned.
const EVAL_CANCEL_GRACE: Duration = Duration::from_secs(2);
const MAX_EVAL_OPERATIONS: u64 = 1_000_000;
const MAX_EVAL_STRING: usize = 4 * 1024 * 1024;
const MAX_EVAL_ARRAY: usize = 100_000;
// Host replies are bounded before entering Rhai, including aggregate strings
// and nested arrays. Exceeding these limits is a successful omission, never a
// post-dispatch error that could encourage replaying an accepted mutation.
const MAX_REPLY_STRING_BYTES: usize = 64 * 1024;
const MAX_REPLY_NODES: usize = 32_768;
const MAX_REPLY_DEPTH: usize = 32;
const MAX_EVAL_CALL_LEVELS: usize = 32;
const MAX_EVAL_EXPR_DEPTH: usize = 64;
const MAX_EVAL_EXPR_DEPTH_IN_FUNCTION: usize = 32;
/// `print`/`debug` text kept for the caller, per eval.
const MAX_EVAL_OUTPUT_CHARS: usize = 64 * 1024;

const HELP: &str = "\
Boss script bindings — every call runs one BossOperation and returns its
payload (compact acknowledgements, bounded data, ids); Saved operations return (). Variables
exist only during this script invocation; use named memory buckets for durable state.

  view()                                    state digest: revision, sessionId, counts
  roster()                                  compact employee status digest
  context()                                 work digest string
  automation(#{type:list|create|update|delete|pause|resume,...})
                                            bounded list document; mutations return accepted + automationId
  summon(#{personaId?,jobTitle,prompt,project,provider?,model?,reasoningEffort?,workspace?,baseBranch?,adoptWorktree?,workGoal?,icon?,resources?,allowBurst?,groupId?,priority?,outcomeId?,newOutcome?,afterSuccess?,finishesOutcome?,prerequisites?,plan?,assignmentKind?,purpose?,item?,requestId?})
                                            employee session id — icon overrides the
                                            persona icon for this employee; resources
                                            declares host-resource needs for the job's
                                            lifetime (#{native_builds:1} for a device
                                            build) and a contested set queues instead
                                            of erroring; workspace \"adopt\" +
                                            adoptWorktree hands a finished employee's
                                            worktree to the new employee, uncommitted
                                            state intact; summons sharing a groupId
                                            form a wave: one notice lands when every
                                            member finishes, fails, or is cancelled;
                                            outcomeId (or a newOutcome spec) assigns the
                                            job to a daemon-owned outcome — its success
                                            leaves a durable handoff with afterSuccess
                                            intent — finishesOutcome makes it the
                                            finisher whose success completes the outcome
                                            silently, and prerequisites holds it queued
                                            until the named siblings finish;
                                            assignmentKind: \"research\" labels
                                            read-only work on draft plans; the
                                            default \"implementation\" requires
                                            the exact approved plan revision
                                            Set purpose: \"building\" for lasting
                                            changes or \"helping\" for research,
                                            reviews, checks, monitoring and chores.
                                            When purpose is omitted, the selected persona
                                            default wins; otherwise explicit
                                            assignmentKind defaults implementation
                                            to Building and research to Helping.
                                            Explicit Helping may use implementation
                                            access for chores. New summons must supply
                                            purpose, a persona default, or explicit assignmentKind.
                                            Legacy records remain unclassified until
                                            setPurpose classifies them; never infer from titles,
                                            workGoal, finisher status or access defaults.
  control(sessionId, \"stop\")               shorthand for a bare action
  control(sessionId, #{type:prompt|steer|stop|startNow|setModel|setPermissions|
                        setWorkspace|setResources|setPlan|setPurpose,...})
                                            setWorkspace takes workspace:
                                            \"local\"|\"worktree\" plus baseBranch
                                            for worktree — stops, rebinds, resumes;
                                            setModel takes provider + model +
                                            optional reasoningEffort — one atomic
                                            reconfigure: an open turn stops
                                            intentionally (never marked failed),
                                            the selection applies, and the
                                            assignment resumes on it; a
                                            provider+model change re-enters
                                            admission on the new pool;
                                            setResources takes a resource map like
                                            summon — a waiting ticket re-admits on
                                            the new set, a running employee swaps
                                            once capacity frees; setPlan re-tags
                                            the employee's plan and item links —
                                            omit keeps, () clears, a value re-tags;
                                            prompt takes an
                                            optional delivery — \"interrupt\" (the
                                            default) steers into an open turn and
                                            queues otherwise, \"queue\" parks behind
                                            the current work, \"steer\" requires a
                                            live turn; steer takes an
                                            optional jobTitle that retitles the
                                            job when the steer redirects the
                                            assignment — label only, no prompt
  resume(sessionId)                         revive an expired employee in place —
                                            queues a synthesized \"verify and
                                            continue\" prompt ahead of whatever
                                            parked at expiry; transcript,
                                            workspace, and cursor survive
  transcript(sessionId[, turn])             transcript map
  projects()                                canonical registered catalog —
                                            [{id,name,path,temporary}]
  resolveProject(selector | #{id|name|path})  one canonical record — bare
                                            text reads by shape: UUID → id,
                                            absolute path → root, else exact
                                            name; the map selects a kind
                                            explicitly. Rejects with a stable
                                            code: project_unknown |
                                            project_ambiguous |
                                            project_selector_invalid |
                                            project_temporary
  readFile(path)                            #{path, content}
  writeFile(path, content)
  listFiles([path])                         [{path,directory}] — root when omitted
  createFolder(path)
  publishDeliverable(path[, name])
  dismissDeliverable(id)
  speak(parts | \"whole utterance\")         client connections reached
  browse(url[, title])                     open an http(s) page in the boss chat panel
  terminal(title, cwd[, command])           pinned standalone terminal request
  upsertPersona(#{name,markdown,defaultPurpose?,...})       id/pinnedFiles/permissions default;
                                            icon takes an employee icon — omit
                                            keeps, () clears. defaultPurpose accepts
                                            building/helping; omit keeps, () clears
  setEmployeeIcon(sessionId, icon | ())
  rename(name)                              the boss's name
  renameEmployee(sessionId, name)
  regenerateAvatar([sessionId])             omit for the boss's own face
  memory(#{type,...})                       bucket op: listBuckets/createBucket/overview/
                                            record/submitSummary/scan/zoomBucket; every
                                            content op names its bucket explicitly
  op(#{type,...})                           advanced escape hatch for supported
                                            Boss operations, tagged script result —
                                            covers createOutcome / setOutcomeState /
                                            resolveHandoff / setOutcomeWaiting /
                                            attachPlan for outcome bookkeeping
  help()                                    this text

State mutations return #{accepted:true,revision,...ids}; chain create includes
chainId/requestId, and chain show returns only that chain. view() returns counts
for chains/personas/employees/retiredEmployees/outcomes/planning/deliverables/waves.
Other host replies exceeding 64 KiB aggregate string bytes, 32768 JSON nodes or
32 nesting levels return #{dispatched:true,resultOmitted:true,...limits}.
Inspect omitted data with direct CLI commands; do not replay the mutation.
Scripts are sequential, not transactions: an error stops later calls but does
not roll back earlier accepted operations. Print acknowledgements as you go;
on failure inspect their ids/revisions before retrying. Reuse the identical
chain requestId and definition under the same supervisor for safe create retries.";

/// The result of one `run` call: the script's scope when it could be
/// recovered, the JSON value it returned (or its error), and the bounded
/// `print`/`debug` output it emitted.
pub struct EvalOutcome {
    pub scope: Option<Scope<'static>>,
    pub value: Result<serde_json::Value, String>,
    pub output: String,
}

enum EvalMessage {
    Call {
        operation: serde_json::Value,
        reply: SyncSender<Result<serde_json::Value, String>>,
    },
    Done {
        scope: Scope<'static>,
        value: Result<serde_json::Value, String>,
    },
}

/// Run `script` against `scope`, dispatching the bound functions'
/// operations through `dispatch` on the calling thread. `dispatch` never
/// sees nested `eval` operations — the caller rejects them before this is
/// reached only for the typed path, so this guards the generic `op` too.
pub fn run(
    scope: Scope<'static>,
    script: &str,
    dispatch: &dyn Fn(BossOperation) -> anyhow::Result<BossResult>,
) -> EvalOutcome {
    let (tx, rx) = std::sync::mpsc::channel::<EvalMessage>();
    let cancelled = Arc::new(AtomicBool::new(false));
    let output = Arc::new(Mutex::new(String::new()));
    let worker = std::thread::Builder::new().name("boss-eval".into()).spawn({
        let cancelled = cancelled.clone();
        let output = output.clone();
        let script = script.to_owned();
        move || run_script(scope, script, tx, cancelled, output)
    });
    let Ok(worker) = worker else {
        return EvalOutcome {
            scope: None,
            value: Err("could not start the eval worker".into()),
            output: String::new(),
        };
    };
    drop(worker);
    let deadline = Instant::now() + EVAL_WALL_CLOCK;
    let mut scope_out = None;
    let value = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break Err(format!(
                "boss eval exceeded its {}s budget",
                EVAL_WALL_CLOCK.as_secs()
            ));
        }
        match rx.recv_timeout(remaining) {
            Ok(EvalMessage::Call { operation, reply }) => {
                let parsed: Result<BossOperation, _> = serde_path_to_error::deserialize(operation);
                let answer = parsed
                    .context("not a Boss operation")
                    .and_then(|operation| {
                        if matches!(operation, BossOperation::Eval { .. }) {
                            bail!("Boss scripts cannot invoke another script");
                        }
                        if matches!(operation,
                            BossOperation::PinDeliverable { .. }
                            | BossOperation::SweepDeliverable { .. }
                            | BossOperation::ArchiveDeliverable { .. }
                            | BossOperation::MarkDeliverableViewed { .. }
                            | BossOperation::MarkGoalsViewed
                            | BossOperation::MarkEmployeeViewed { .. }
                            | BossOperation::SetEmployeeAttention { .. }
                        ) {
                            bail!("sidebar presentation state is client-owned and unavailable to scripts");
                        }
                        let result = dispatch(operation.clone())?;
                        Ok(script_reply(&operation, result))
                    })
                    .map_err(|error| format!("{error:#}"));
                // A vanished worker means the script already failed.
                let _ = reply.send(answer);
            }
            Ok(EvalMessage::Done { scope, value }) => {
                scope_out = Some(scope);
                break value;
            }
            Err(RecvTimeoutError::Disconnected) => {
                break Err("the eval worker stopped without a result".into());
            }
            Err(RecvTimeoutError::Timeout) => {
                cancelled.store(true, Ordering::Relaxed);
                match rx.recv_timeout(EVAL_CANCEL_GRACE) {
                    Ok(EvalMessage::Done { scope, value }) => {
                        scope_out = Some(scope);
                        break value;
                    }
                    _ => {
                        break Err(format!(
                            "boss eval exceeded its {}s budget",
                            EVAL_WALL_CLOCK.as_secs()
                        ));
                    }
                }
            }
        }
    };
    EvalOutcome {
        scope: scope_out,
        value,
        output: std::mem::take(&mut *output.lock()),
    }
}

/// Script-only projection: direct commands and clients retain their wire
/// results. Never turn a successful dispatch into a response-size error.
fn script_reply(operation: &BossOperation, result: BossResult) -> serde_json::Value {
    use serde_json::json;
    use waku_protocol::boss::AutomationOperation;
    use waku_protocol::chains::ChainOperation;

    let mut reply = match result {
        BossResult::State { state } => match operation {
            BossOperation::View => json!({
                "type": "stateDigest",
                "digest": {
                    "revision": state.revision,
                    "sessionId": state.session_id,
                    "counts": {
                        "chains": state.chains.len(),
                        "personas": state.personas.len(),
                        "employees": state.employees.len(),
                        "retiredEmployees": state.retired_employees.len(),
                        "outcomes": state.outcomes.len(),
                        "planning": state.planning.len(),
                        "deliverables": state.deliverables.len(),
                        "waves": state.waves.len(),
                    },
                },
            }),
            BossOperation::Chain { action } => {
                let chain = state.chains.iter().find(|chain| match action {
                    ChainOperation::Create { request_id, .. } => chain.request_id == *request_id,
                    ChainOperation::Show { chain_id }
                    | ChainOperation::Control { chain_id, .. }
                    | ChainOperation::Revise { chain_id, .. }
                    | ChainOperation::Approve { chain_id, .. }
                    | ChainOperation::RecordResult { chain_id, .. } => chain.id == *chain_id,
                });
                if matches!(action, ChainOperation::Show { .. }) {
                    json!({"type": "chain", "chain": chain})
                } else {
                    json!({
                        "type": "accepted", "accepted": true,
                        "chainId": chain.map(|chain| chain.id),
                        "requestId": chain.map(|chain| chain.request_id),
                        "revision": chain.map(|chain| chain.revision),
                        "definitionVersion": chain.map(|chain| chain.definition_version),
                        "status": chain.map(|chain| chain.status),
                    })
                }
            }
            _ => {
                let mut ack =
                    json!({"type": "accepted", "accepted": true, "revision": state.revision});
                match operation {
                    BossOperation::CreateOutcome { outcome, .. } => {
                        ack["outcomeId"] = json!(
                            state
                                .outcomes
                                .iter()
                                .rev()
                                .find(|entry| entry.outcome == outcome.trim())
                                .map(|entry| entry.id)
                        );
                    }
                    BossOperation::SetOutcomeState { outcome, .. }
                    | BossOperation::ResolveHandoff { outcome, .. }
                    | BossOperation::SetOutcomeWaiting { outcome, .. }
                    | BossOperation::AttachPlan { outcome, .. } => {
                        ack["outcomeId"] = json!(outcome)
                    }
                    BossOperation::UpsertPersona { persona } => {
                        ack["personaId"] = json!(
                            state
                                .personas
                                .iter()
                                .find(|entry| if persona.id.is_nil() {
                                    entry.name == persona.name.trim()
                                } else {
                                    entry.id == persona.id
                                })
                                .map(|entry| entry.id)
                        );
                    }
                    BossOperation::DeletePersona { persona_id } => {
                        ack["personaId"] = json!(persona_id)
                    }
                    BossOperation::RenameEmployee { session_id, .. }
                    | BossOperation::SetEmployeeIcon { session_id, .. } => {
                        ack["sessionId"] = json!(session_id)
                    }
                    BossOperation::UpdatePlanItems { plan, .. }
                    | BossOperation::SetPlanItemState { plan, .. }
                    | BossOperation::SetPlanOutcome { plan, .. } => ack["planId"] = json!(plan),
                    _ => {}
                }
                ack
            }
        },
        BossResult::Automations { state }
            if !matches!(
                operation,
                BossOperation::Automation {
                    action: AutomationOperation::List
                }
            ) =>
        {
            let id = match operation {
                BossOperation::Automation { action } => match action {
                    AutomationOperation::Create { input } => state
                        .automations
                        .iter()
                        .rev()
                        .find(|entry| entry.name == input.name)
                        .map(|entry| entry.id),
                    AutomationOperation::Update { input } => input.id,
                    AutomationOperation::Delete { automation_id }
                    | AutomationOperation::Pause { automation_id }
                    | AutomationOperation::Resume { automation_id } => Some(*automation_id),
                    AutomationOperation::List => None,
                },
                _ => None,
            };
            json!({"type": "accepted", "accepted": true, "automationId": id})
        }
        other => serde_json::to_value(other).unwrap_or_else(|_| {
            json!({
                "type": "resultOmitted", "dispatched": true, "resultOmitted": true,
            })
        }),
    };
    let mut bytes = MAX_REPLY_STRING_BYTES;
    let mut nodes = MAX_REPLY_NODES;
    if !reply_fits(&reply, 0, &mut bytes, &mut nodes) {
        // Do not truncate silently or throw after a committed effect. The
        // caller can inspect through a direct command instead of replaying.
        reply = json!({
            "type": "resultOmitted", "dispatched": true, "resultOmitted": true,
            "resultType": reply.get("type"),
            "maxStringBytes": MAX_REPLY_STRING_BYTES,
            "maxNodes": MAX_REPLY_NODES,
            "maxDepth": MAX_REPLY_DEPTH,
        });
    }
    reply
}

fn reply_fits(
    value: &serde_json::Value,
    depth: usize,
    bytes: &mut usize,
    nodes: &mut usize,
) -> bool {
    if depth > MAX_REPLY_DEPTH || *nodes == 0 {
        return false;
    }
    *nodes -= 1;
    let mut take_bytes = |length| match bytes.checked_sub(length) {
        Some(remaining) => {
            *bytes = remaining;
            true
        }
        None => false,
    };
    match value {
        serde_json::Value::String(text) => take_bytes(text.len()),
        serde_json::Value::Array(items) => items
            .iter()
            .all(|item| reply_fits(item, depth + 1, bytes, nodes)),
        serde_json::Value::Object(fields) => fields.iter().all(|(key, value)| {
            let Some(remaining) = bytes.checked_sub(key.len()) else {
                return false;
            };
            *bytes = remaining;
            reply_fits(value, depth + 1, bytes, nodes)
        }),
        _ => true,
    }
}

fn run_script(
    mut scope: Scope<'static>,
    script: String,
    tx: Sender<EvalMessage>,
    cancelled: Arc<AtomicBool>,
    output: Arc<Mutex<String>>,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut engine = Engine::new();
        engine
            .set_max_operations(MAX_EVAL_OPERATIONS)
            .set_max_string_size(MAX_EVAL_STRING)
            .set_max_array_size(MAX_EVAL_ARRAY)
            // Host replies have their own aggregate budget in script_reply.
            // Script-built maps stay bounded by operations and wall clock.
            .set_max_map_size(0)
            .set_max_call_levels(MAX_EVAL_CALL_LEVELS)
            .set_max_expr_depths(MAX_EVAL_EXPR_DEPTH, MAX_EVAL_EXPR_DEPTH_IN_FUNCTION)
            .set_max_modules(0);
        engine.on_progress(move |_| {
            cancelled
                .load(Ordering::Relaxed)
                .then(|| Dynamic::from("boss eval was cancelled"))
        });
        engine.on_print({
            let output = output.clone();
            move |text| append_output(&output, text)
        });
        engine.on_debug({
            let output = output.clone();
            move |text, source, _| {
                append_output(
                    &output,
                    &match source {
                        Some(source) => format!("{text} @ {source}"),
                        None => text.to_owned(),
                    },
                )
            }
        });
        bind(&mut engine, &tx);
        match engine.compile(&script) {
            Err(error) => Err(format!("eval script did not parse: {error}")),
            Ok(ast) => engine
                .eval_ast_with_scope::<Dynamic>(&mut scope, &ast)
                .map(|value| dynamic_to_json(&value))
                .map_err(|error| error.to_string()),
        }
    }));
    let value = match result {
        Ok(value) => value,
        Err(panic) => Err(format!(
            "eval script panicked: {}",
            panic
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic")
        )),
    };
    let _ = tx.send(EvalMessage::Done { scope, value });
}

fn append_output(output: &Mutex<String>, text: &str) {
    let mut buffer = output.lock();
    let remaining = MAX_EVAL_OUTPUT_CHARS.saturating_sub(buffer.len());
    if remaining == 0 {
        return;
    }
    let mut end = text.len().min(remaining);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    buffer.push_str(&text[..end]);
    if end < text.len() {
        buffer.push_str("…");
    }
}

/// Bind the boss operations as script-callable functions. Each builds the
/// operation's wire JSON — the same shapes `goddard-agent boss` accepts —
/// so the eval surface tracks the protocol enum.
fn terminal_call(
    tx: &Sender<EvalMessage>,
    title: ImmutableString,
    cwd: ImmutableString,
    command: Option<String>,
) -> Result<Dynamic, Box<EvalAltResult>> {
    call(
        tx,
        tagged(
            "terminal",
            serde_json::json!({ "title": title.as_str(), "cwd": cwd.as_str(), "command": command }),
        ),
    )
    .and_then(unwrap_result)
}

fn bind(engine: &mut Engine, tx: &Sender<EvalMessage>) {
    engine.register_fn("automation", {
        let tx = tx.clone();
        move |action: Map| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                serde_json::json!({
                    "type": "automation",
                    "action": dynamic_to_json(&Dynamic::from_map(action)),
                }),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("view", {
        let tx = tx.clone();
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            call(&tx, tagged("view", serde_json::json!({}))).and_then(unwrap_result)
        }
    });
    engine.register_fn("roster", {
        let tx = tx.clone();
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            call(&tx, tagged("roster", serde_json::json!({}))).and_then(unwrap_result)
        }
    });
    engine.register_fn("context", {
        let tx = tx.clone();
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            call(&tx, tagged("context", serde_json::json!({}))).and_then(unwrap_result)
        }
    });
    engine.register_fn("terminal", {
        let tx = tx.clone();
        move |title: ImmutableString, cwd: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            terminal_call(&tx, title, cwd, None)
        }
    });
    engine.register_fn("terminal", {
        let tx = tx.clone();
        move |title: ImmutableString,
              cwd: ImmutableString,
              command: ImmutableString|
              -> Result<Dynamic, Box<EvalAltResult>> {
            terminal_call(&tx, title, cwd, Some(command.to_string()))
        }
    });
    engine.register_fn("summon", {
        let tx = tx.clone();
        move |args: Map| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged("summon", dynamic_to_json(&Dynamic::from_map(args))),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("setResourcePolicy", {
        let tx = tx.clone();
        move |args: Map| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged(
                    "setResourcePolicy",
                    dynamic_to_json(&Dynamic::from_map(args)),
                ),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("control", {
        let tx = tx.clone();
        move |session_id: ImmutableString, action: Dynamic| -> Result<Dynamic, Box<EvalAltResult>> {
            // A bare action name is shorthand for its field-less action.
            let action = match action.clone().into_immutable_string() {
                Ok(name) => serde_json::json!({ "type": name.as_str() }),
                Err(_) => dynamic_to_json(&action),
            };
            call(
                &tx,
                serde_json::json!({
                    "type": "control",
                    "sessionId": session_id.as_str(),
                    "action": action,
                }),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("resume", {
        let tx = tx.clone();
        move |session_id: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged(
                    "resume",
                    serde_json::json!({ "sessionId": session_id.as_str() }),
                ),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("transcript", {
        let tx = tx.clone();
        move |session_id: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged(
                    "transcript",
                    serde_json::json!({ "sessionId": session_id.as_str() }),
                ),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("transcript", {
        let tx = tx.clone();
        move |session_id: ImmutableString, turn: i64| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged(
                    "transcript",
                    serde_json::json!({ "sessionId": session_id.as_str(), "turn": turn }),
                ),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("projects", {
        let tx = tx.clone();
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                serde_json::json!({
                    "type": "projects",
                    "action": { "type": "list" },
                }),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("resolveProject", {
        let tx = tx.clone();
        move |selector: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                serde_json::json!({
                    "type": "projects",
                    "action": { "type": "resolve", "selector": selector.as_str() },
                }),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("resolveProject", {
        let tx = tx.clone();
        move |selector: Map| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                serde_json::json!({
                    "type": "projects",
                    "action": {
                        "type": "resolve",
                        "selector": dynamic_to_json(&Dynamic::from_map(selector)),
                    },
                }),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("readFile", {
        let tx = tx.clone();
        move |path: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(&tx, tagged("readFile", json_path(path))).and_then(unwrap_result)
        }
    });
    engine.register_fn("writeFile", {
        let tx = tx.clone();
        move |path: ImmutableString,
              content: ImmutableString|
              -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged(
                    "writeFile",
                    serde_json::json!({ "path": path.as_str(), "content": content.as_str() }),
                ),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("listFiles", {
        let tx = tx.clone();
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            call(&tx, tagged("listFiles", serde_json::json!({ "path": "" })))
                .and_then(unwrap_result)
        }
    });
    engine.register_fn("listFiles", {
        let tx = tx.clone();
        move |path: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(&tx, tagged("listFiles", json_path(path))).and_then(unwrap_result)
        }
    });
    engine.register_fn("createFolder", {
        let tx = tx.clone();
        move |path: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(&tx, tagged("createFolder", json_path(path))).and_then(unwrap_result)
        }
    });
    engine.register_fn("publishDeliverable", {
        let tx = tx.clone();
        move |path: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(&tx, tagged("publishDeliverable", json_path(path))).and_then(unwrap_result)
        }
    });
    engine.register_fn("publishDeliverable", {
        let tx = tx.clone();
        move |path: ImmutableString, name: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged(
                    "publishDeliverable",
                    serde_json::json!({ "path": path.as_str(), "name": name.as_str() }),
                ),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("dismissDeliverable", {
        let tx = tx.clone();
        move |id: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged(
                    "dismissDeliverable",
                    serde_json::json!({ "id": id.as_str() }),
                ),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("speak", {
        let tx = tx.clone();
        move |parts: Dynamic| -> Result<Dynamic, Box<EvalAltResult>> {
            let parts = match parts.clone().into_immutable_string() {
                Ok(part) => serde_json::json!([part.as_str()]),
                Err(_) => dynamic_to_json(&parts),
            };
            call(&tx, tagged("speak", serde_json::json!({ "parts": parts })))
                .and_then(unwrap_result)
        }
    });
    engine.register_fn("browse", {
        let tx = tx.clone();
        move |url: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged("browse", serde_json::json!({ "url": url.as_str() })),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("browse", {
        let tx = tx.clone();
        move |url: ImmutableString, title: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged(
                    "browse",
                    serde_json::json!({ "url": url.as_str(), "title": title.as_str() }),
                ),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("upsertPersona", {
        let tx = tx.clone();
        move |persona: Map| -> Result<Dynamic, Box<EvalAltResult>> {
            // Creation needs only the meaningful fields; the daemon
            // assigns a fresh id to a nil one on upsert.
            let mut merged = serde_json::json!({
                "id": uuid::Uuid::nil(),
                "pinnedFiles": [],
                "permissions": {},
            });
            if let (serde_json::Value::Object(base), serde_json::Value::Object(extra)) =
                (&mut merged, dynamic_to_json(&Dynamic::from_map(persona)))
            {
                base.extend(extra);
            }
            call(
                &tx,
                tagged("upsertPersona", serde_json::json!({ "persona": merged })),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("setEmployeeIcon", {
        let tx = tx.clone();
        move |session_id: ImmutableString, icon: Dynamic| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged(
                    "setEmployeeIcon",
                    serde_json::json!({
                        "sessionId": session_id.as_str(),
                        "icon": dynamic_to_json(&icon),
                    }),
                ),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("rename", {
        let tx = tx.clone();
        move |name: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged("rename", serde_json::json!({ "name": name.as_str() })),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("renameEmployee", {
        let tx = tx.clone();
        move |session_id: ImmutableString,
              name: ImmutableString|
              -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged(
                    "renameEmployee",
                    serde_json::json!({
                        "sessionId": session_id.as_str(),
                        "name": name.as_str(),
                    }),
                ),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("regenerateAvatar", {
        let tx = tx.clone();
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged("regenerateAvatar", serde_json::json!({ "sessionId": null })),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("regenerateAvatar", {
        let tx = tx.clone();
        move |session_id: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                tagged(
                    "regenerateAvatar",
                    serde_json::json!({ "sessionId": session_id.as_str() }),
                ),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("memory", {
        let tx = tx.clone();
        move |operation: Map| -> Result<Dynamic, Box<EvalAltResult>> {
            call(
                &tx,
                serde_json::json!({
                    "type": "memory",
                    "operation": dynamic_to_json(&Dynamic::from_map(operation)),
                }),
            )
            .and_then(unwrap_result)
        }
    });
    engine.register_fn("op", {
        let tx = tx.clone();
        move |operation: Map| -> Result<Dynamic, Box<EvalAltResult>> {
            // The escape hatch returns the whole serialized BossResult —
            // type tag included — since its shape varies by operation.
            call(&tx, dynamic_to_json(&Dynamic::from_map(operation))).map(json_to_dynamic)
        }
    });
    engine.register_fn("help", move || HELP);
}

fn json_path(path: ImmutableString) -> serde_json::Value {
    serde_json::json!({ "path": path.as_str() })
}

/// Build one operation's wire JSON: the `type` tag plus the argument map's
/// fields merged over it — the same object `goddard-agent boss` takes.
fn tagged(tag: &str, args: serde_json::Value) -> serde_json::Value {
    let mut object = serde_json::Map::with_capacity(8);
    object.insert("type".into(), tag.into());
    if let serde_json::Value::Object(fields) = args {
        object.extend(fields);
    }
    serde_json::Value::Object(object)
}

/// Ship an operation to the dispatcher thread and block on its serialized
/// result. A closed channel means the eval was cancelled or finished.
fn call(
    tx: &Sender<EvalMessage>,
    operation: serde_json::Value,
) -> Result<serde_json::Value, Box<EvalAltResult>> {
    let (reply_tx, reply_rx) = sync_channel::<Result<serde_json::Value, String>>(1);
    tx.send(EvalMessage::Call {
        operation,
        reply: reply_tx,
    })
    .map_err(|_| boxed_err("the boss eval dispatcher is gone"))?;
    reply_rx
        .recv()
        .map_err(|_| boxed_err("the boss eval dispatcher is gone"))?
        .map_err(boxed_err)
}

/// A `BossResult`'s payload field — `{"type":"state","state":{...}}`
/// unwraps to the state map, `Saved` to `()`. Multi-payload results keep
/// their remaining fields as a map.
fn unwrap_result(result: serde_json::Value) -> Result<Dynamic, Box<EvalAltResult>> {
    let serde_json::Value::Object(mut object) = result else {
        return Ok(json_to_dynamic(result));
    };
    if object.get("type").and_then(|value| value.as_str()) == Some("planRejected") {
        let error = object.get("error").cloned().unwrap_or_default();
        return Err(format!("plan assignment rejected: {error}").into());
    }
    if object.get("type").and_then(|value| value.as_str()) == Some("projectRejected") {
        let error = object.get("error").cloned().unwrap_or_default();
        return Err(format!("project selection rejected: {error}").into());
    }
    object.remove("type");
    Ok(match object.len() {
        0 => Dynamic::UNIT,
        1 => json_to_dynamic(
            object
                .into_values()
                .next()
                .unwrap_or(serde_json::Value::Null),
        ),
        _ => json_to_dynamic(serde_json::Value::Object(object)),
    })
}

fn boxed_err(message: impl Into<String>) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(
        message.into().into(),
        Position::NONE,
    ))
}

fn json_to_dynamic(value: serde_json::Value) -> Dynamic {
    match value {
        serde_json::Value::Null => Dynamic::UNIT,
        serde_json::Value::Bool(value) => value.into(),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                value.into()
            } else if let Some(value) = number.as_u64() {
                i64::try_from(value)
                    .map_or_else(|_| Dynamic::from(value as f64), Into::<Dynamic>::into)
            } else {
                number.as_f64().unwrap_or_default().into()
            }
        }
        serde_json::Value::String(value) => value.into(),
        serde_json::Value::Array(items) => {
            Dynamic::from_array(items.into_iter().map(json_to_dynamic).collect())
        }
        serde_json::Value::Object(fields) => Dynamic::from_map(
            fields
                .into_iter()
                .map(|(key, value)| (key.into(), json_to_dynamic(value)))
                .collect(),
        ),
    }
}

fn dynamic_to_json(value: &Dynamic) -> serde_json::Value {
    if value.is_unit() {
        serde_json::Value::Null
    } else if let Ok(value) = value.as_bool() {
        value.into()
    } else if let Ok(value) = value.as_int() {
        value.into()
    } else if let Ok(value) = value.as_float() {
        serde_json::Number::from_f64(value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number)
    } else if let Some(text) = value
        .read_lock::<ImmutableString>()
        .map(|text| text.as_str().to_owned())
    {
        text.into()
    } else if let Some(array) = value.read_lock::<Array>() {
        serde_json::Value::Array(array.iter().map(dynamic_to_json).collect())
    } else if let Some(map) = value.read_lock::<Map>() {
        serde_json::Value::Object(
            map.iter()
                .map(|(key, value)| (key.to_string(), dynamic_to_json(value)))
                .collect(),
        )
    } else {
        serde_json::Value::String(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dispatch_ok(operation: BossOperation) -> anyhow::Result<BossResult> {
        match operation {
            BossOperation::View => Ok(BossResult::Saved),
            other => bail!("unexpected operation {other:?}"),
        }
    }

    fn eval(script: &str) -> EvalOutcome {
        run(Scope::new(), script, &dispatch_ok)
    }

    fn eval_with(
        script: &str,
        scope: Scope<'static>,
        dispatch: &dyn Fn(BossOperation) -> anyhow::Result<BossResult>,
    ) -> EvalOutcome {
        run(scope, script, dispatch)
    }

    #[test]
    fn plain_scripts_return_json_and_keep_variables() {
        let outcome = eval("40 + 2");
        assert_eq!(outcome.value.as_ref(), Ok(&serde_json::json!(42)));
        assert!(outcome.scope.is_some());

        let outcome = eval_with("let answer = 42; answer", Scope::new(), &dispatch_ok);
        assert_eq!(outcome.value.as_ref(), Ok(&serde_json::json!(42)));
        let scope = outcome.scope.expect("scope comes back");

        let outcome = eval_with("answer + 1", scope, &dispatch_ok);
        assert_eq!(outcome.value.as_ref(), Ok(&serde_json::json!(43)));
    }

    #[test]
    fn browse_binding_dispatches_url_and_optional_title() {
        let outcome = eval_with(
            "browse(\"https://example.com\", \"Docs\")",
            Scope::new(),
            &|operation| match operation {
                BossOperation::Browse { url, title } => Ok(BossResult::Browse {
                    session_id: uuid::Uuid::nil(),
                    url,
                    title,
                }),
                other => bail!("unexpected operation {other:?}"),
            },
        );
        let value = outcome.value.unwrap();
        assert_eq!(value["url"], "https://example.com");
        assert_eq!(value["title"], "Docs");
    }

    #[test]
    fn bound_operations_dispatch_and_unwrap() {
        let outcome = eval_with("let seen = view(); seen == ()", Scope::new(), &dispatch_ok);
        assert_eq!(outcome.value.as_ref(), Ok(&serde_json::json!(true)));

        let outcome = eval_with("roster()", Scope::new(), &|operation| match operation {
            BossOperation::Roster => Ok(BossResult::Roster {
                roster: "1 live".into(),
            }),
            other => bail!("unexpected operation {other:?}"),
        });
        assert_eq!(outcome.value.as_ref(), Ok(&serde_json::json!("1 live")));

        let outcome = eval_with(
            "op(#{type: \"dismissDeliverable\", id: \"00000000-0000-0000-0000-000000000000\"})",
            Scope::new(),
            &|_| bail!("dispatch rejects it"),
        );
        assert!(
            outcome
                .value
                .as_ref()
                .unwrap_err()
                .contains("dispatch rejects it")
        );
    }

    #[test]
    fn op_maps_reach_dispatch_and_oversized_documents_are_omitted() {
        // The documented minimal attachPlan/resolveHandoff forms carry two
        // UUIDs (and a nested decision map); building the map field by field
        // reaches the same boundary. A size rejection is the regression —
        // the dispatch's own validation error proves the map got through.
        for script in [
            r#"op(#{type:"attachPlan", outcome:"d7adf75c-9f00-46f6-b952-8851243bc923", plan:"a464137f-21e1-4800-91d2-6001223eb1d4"})"#,
            r#"op(#{type:"resolveHandoff", outcome:"d7adf75c-9f00-46f6-b952-8851243bc923", handoff:"3d7b5f5a-a7ab-4370-ac3d-038e43bd2361", decision:#{type:"dismiss"}})"#,
            r#"let m = #{}; m.type = "attachPlan"; m.outcome = "d7adf75c-9f00-46f6-b952-8851243bc923"; m.plan = "a464137f-21e1-4800-91d2-6001223eb1d4"; op(m)"#,
        ] {
            let outcome = eval_with(script, Scope::new(), &|_| bail!("dispatch rejects it"));
            let error = outcome.value.as_ref().unwrap_err();
            assert!(
                error.contains("dispatch rejects it"),
                "op rejected its operation map before dispatch: {error}"
            );
        }

        // A large listing is explicitly omitted instead of being rejected
        // by Rhai after dispatch.
        let outcome = eval_with(
            r#"op(#{type: "view"}).resultOmitted"#,
            Scope::new(),
            &|_| {
                Ok(BossResult::Files {
                    files: (0..6_000)
                        .map(|index| waku_protocol::boss::BossFile {
                            path: format!("f{index}.md"),
                            directory: false,
                        })
                        .collect(),
                })
            },
        );
        assert_eq!(outcome.value.as_ref(), Ok(&serde_json::json!(true)));
    }

    #[test]
    fn project_discovery_bindings_dispatch_and_surface_rejection() {
        use waku_protocol::projects::{
            CanonicalProject, ProjectOperation, ProjectSelectionError, ProjectSelectionErrorCode,
            ProjectSelector, ProjectSelectorFields,
        };
        let record = CanonicalProject {
            id: uuid::Uuid::nil(),
            name: "alpha".into(),
            path: "/work/alpha".into(),
            temporary: false,
        };
        let outcome = eval_with("projects()", Scope::new(), &|operation| match operation {
            BossOperation::Projects {
                action: ProjectOperation::List,
            } => Ok(BossResult::Projects {
                projects: vec![record.clone()],
            }),
            other => bail!("unexpected operation {other:?}"),
        });
        assert_eq!(
            outcome.value.unwrap()[0]["id"],
            serde_json::json!(record.id.to_string())
        );

        // Bare text and a kinded map reach the same resolve operation.
        for (script, expected) in [
            (
                r#"resolveProject("alpha")"#,
                ProjectSelector::Bare("alpha".into()),
            ),
            (
                r#"resolveProject(#{name: "alpha"})"#,
                ProjectSelector::Kinded(ProjectSelectorFields {
                    name: Some("alpha".into()),
                    ..Default::default()
                }),
            ),
        ] {
            let record = record.clone();
            let outcome = eval_with(script, Scope::new(), &|operation| match operation {
                BossOperation::Projects {
                    action: ProjectOperation::Resolve { selector },
                } => {
                    assert_eq!(selector, expected);
                    Ok(BossResult::Project {
                        project: record.clone(),
                    })
                }
                other => bail!("unexpected operation {other:?}"),
            });
            assert_eq!(outcome.value.unwrap()["name"], "alpha");
        }

        // A coded rejection surfaces as a script error carrying the code.
        let outcome = eval_with(r#"resolveProject("missing")"#, Scope::new(), &|_| {
            Ok(BossResult::ProjectRejected {
                error: ProjectSelectionError {
                    code: ProjectSelectionErrorCode::ProjectUnknown,
                    selector: "missing".into(),
                    step: None,
                    message: "no registered project named 'missing'".into(),
                    next_action: "run projects()".into(),
                    candidates: Vec::new(),
                    truncated: false,
                },
            })
        });
        let error = outcome.value.unwrap_err();
        assert!(error.contains("project selection rejected"));
        assert!(error.contains("project_unknown"));
    }

    #[test]
    fn terminal_binding_dispatches_optional_command() {
        let outcome = eval_with(
            "terminal(\"Dev server\", \"/work/app\", \"bun run dev\")",
            Scope::new(),
            &|operation| match operation {
                BossOperation::Terminal {
                    title,
                    cwd,
                    command,
                } => {
                    assert_eq!(title, "Dev server");
                    assert_eq!(cwd, "/work/app");
                    assert_eq!(command.as_deref(), Some("bun run dev"));
                    Ok(BossResult::TerminalRequested { title, cwd })
                }
                other => bail!("unexpected operation {other:?}"),
            },
        );
        assert_eq!(
            outcome.value.as_ref(),
            Ok(&serde_json::json!({ "title": "Dev server", "cwd": "/work/app" }))
        );
    }

    #[test]
    fn oversized_reply_after_committed_effect_does_not_abort_later_operations() {
        use std::cell::Cell;
        let committed = Cell::new(0);
        // Reproduce the old boundary: Rhai rejects a native callback's
        // aggregate strings only after its effect has already happened.
        let mut engine = Engine::new();
        engine.set_max_string_size(MAX_EVAL_STRING);
        let raw_committed = Arc::new(AtomicBool::new(false));
        engine.register_fn("commit", {
            let raw_committed = raw_committed.clone();
            move || {
                raw_committed.store(true, Ordering::Relaxed);
                json_to_dynamic(serde_json::json!({
                    "first": "x".repeat(MAX_EVAL_STRING / 2 + 1),
                    "second": "y".repeat(MAX_EVAL_STRING / 2 + 1),
                }))
            }
        });
        let error = engine.eval::<Dynamic>("commit()").unwrap_err();
        assert!(error.to_string().contains("Length of string too large"));
        assert!(raw_committed.load(Ordering::Relaxed));

        let outcome = eval_with(
            r#"let a = op(#{type:"rename", name:"first"});
               let b = op(#{type:"rename", name:"second"});
               #{first:a, second:b}"#,
            Scope::new(),
            &|_| {
                committed.set(committed.get() + 1);
                Ok(BossResult::File {
                    path: "large.md".into(),
                    content: "x".repeat(MAX_EVAL_STRING + 1),
                })
            },
        );
        assert_eq!(committed.get(), 2);
        let value = outcome.value.unwrap();
        for key in ["first", "second"] {
            assert_eq!(value[key]["dispatched"], true);
            assert_eq!(value[key]["resultOmitted"], true);
            assert_eq!(value[key]["resultType"], "file");
        }
    }

    #[test]
    fn oversized_arrays_and_deep_host_data_are_omitted_without_aborting_the_script() {
        let mut deep = serde_json::Value::Null;
        for _ in 0..MAX_REPLY_DEPTH + 1 {
            deep = serde_json::json!({"nested": deep});
        }
        for data in [serde_json::json!(vec![0; MAX_EVAL_ARRAY + 1]), deep] {
            let outcome = eval_with(
                r#"let response = readFile("large.json");
                   let later = op(#{type:"view"});
                   #{omitted:response.resultOmitted,later:later.type}"#,
                Scope::new(),
                &|operation| match operation {
                    BossOperation::ReadFile { .. } => Ok(BossResult::Eval {
                        value: data.clone(),
                        output: String::new(),
                    }),
                    BossOperation::View => Ok(BossResult::Saved),
                    other => bail!("unexpected operation {other:?}"),
                },
            );
            assert_eq!(
                outcome.value.unwrap(),
                serde_json::json!({"omitted":true,"later":"saved"})
            );
        }
    }

    #[test]
    fn state_mutations_return_ids_and_view_digest_even_after_state_outgrows_rhai() {
        let root = std::env::temp_dir().join(format!("boss-eval-{}", uuid::Uuid::new_v4()));
        let service = crate::boss::BossService::open(root.clone()).unwrap();
        service.activate().unwrap();
        service
            .create_outcome(None, &"x".repeat(MAX_EVAL_STRING + 1), "seed")
            .unwrap();
        let outcome = eval_with(
            r#"let a = op(#{type:"createOutcome", outcome:"first", successCriteria:"done"});
               let b = op(#{type:"createOutcome", outcome:"second", successCriteria:"done"});
               #{first:a, second:b, digest:view()}"#,
            Scope::new(),
            &|operation| service.handle(None, operation),
        );
        let value = outcome.value.unwrap();
        assert_eq!(value["first"]["accepted"], true);
        assert_eq!(value["second"]["accepted"], true);
        assert_ne!(value["first"]["outcomeId"], value["second"]["outcomeId"]);
        assert!(value["first"]["outcomeId"].as_str().is_some());
        assert_eq!(value["digest"]["counts"]["outcomes"], 3);
        assert!(serde_json::to_vec(&value).unwrap().len() < 1024);
        assert_eq!(service.document().outcomes.len(), 3);
        drop(service);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn later_failure_keeps_earlier_acknowledgements_and_effects() {
        use std::cell::Cell;
        let committed = Cell::new(0);
        let outcome = eval_with(
            r#"print(op(#{type:"rename",name:"first"}));
               op(#{type:"rename",name:"reject"});
               op(#{type:"rename",name:"unreachable"})"#,
            Scope::new(),
            &|operation| {
                if matches!(operation, BossOperation::Rename { ref name } if name == "reject") {
                    bail!("validation rejected");
                }
                committed.set(committed.get() + 1);
                Ok(BossResult::Saved)
            },
        );
        assert_eq!(committed.get(), 1);
        assert!(outcome.value.unwrap_err().contains("validation rejected"));
        assert!(outcome.output.contains("saved"));
    }

    #[test]
    fn operation_errors_surface_as_script_errors() {
        let outcome = eval_with("readFile(\"memory/nope.md\")", Scope::new(), &|_| {
            bail!("persona does not grant access to this Boss file")
        });
        let error = outcome.value.as_ref().unwrap_err();
        assert!(
            error.contains("persona does not grant access to this Boss file"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn nested_eval_is_rejected_before_dispatch() {
        let outcome = eval_with(
            "op(#{type: \"eval\", script: \"1\"})",
            Scope::new(),
            &dispatch_ok,
        );
        let error = outcome.value.as_ref().unwrap_err();
        assert!(
            error.contains("Boss scripts cannot invoke another script"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn scripts_cannot_mutate_client_owned_sidebar_state() {
        let outcome = eval_with(
            "op(#{type: \"markGoalsViewed\"})",
            Scope::new(),
            &dispatch_ok,
        );
        let error = outcome.value.as_ref().unwrap_err();
        assert!(error.contains("sidebar presentation state is client-owned"));
    }

    #[test]
    fn print_output_is_captured_and_bounded() {
        let outcome = eval("print(\"hello\"); debug(\"world\"); 7");
        assert_eq!(outcome.value.as_ref(), Ok(&serde_json::json!(7)));
        assert!(outcome.output.contains("hello"));
        assert!(outcome.output.contains("world"));
    }

    #[test]
    fn runaway_scripts_are_terminated() {
        let outcome = eval("loop { }");
        let error = outcome.value.as_ref().unwrap_err();
        assert!(
            error.contains("operations") || error.contains("Script terminated"),
            "unexpected error: {error}"
        );
    }
}
