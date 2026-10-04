//! The boss's Rhai eval loop: one script batches `BossOperation`s whose
//! results feed later calls, with variables persisting per boss session.
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

/// Longest script source an `eval` call accepts.
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
const MAX_EVAL_MAP: usize = 10_000;
const MAX_EVAL_CALL_LEVELS: usize = 32;
const MAX_EVAL_EXPR_DEPTH: usize = 64;
const MAX_EVAL_EXPR_DEPTH_IN_FUNCTION: usize = 32;
/// `print`/`debug` text kept for the caller, per eval.
const MAX_EVAL_OUTPUT_CHARS: usize = 64 * 1024;

const HELP: &str = "\
Boss eval bindings — every call runs one BossOperation and returns its
payload (state maps, strings, ids); Saved operations return (). Variables
persist between eval calls for this boss session.

  view()                                    boss state map
  roster()                                  compact employee status digest
  context()                                 work digest string
  automation(#{type:list|create|update|delete|pause|resume,...})
                                            automation document with schedules and run history
  summon(#{personaId,jobTitle,prompt,project,provider?,model?,reasoningEffort?,workspace?,baseBranch?,workGoal?})
                                            employee session id
  control(sessionId, \"stop\")               shorthand for a bare action
  control(sessionId, #{type:prompt|steer|stop|setModel|setPermissions|
                        setWorkspace,...})
                                            setWorkspace takes workspace:
                                            \"local\"|\"worktree\" plus baseBranch
                                            for worktree — stops, rebinds, resumes
  transcript(sessionId[, turn])             transcript map
  readFile(path)                            #{path, content}
  writeFile(path, content)
  listFiles([path])                         [{path,directory}] — root when omitted
  createFolder(path)
  publishDeliverable(path[, name])
  dismissDeliverable(id)
  speak(parts | \"whole utterance\")         client connections reached
  browse(url[, title])                     open an http(s) page in the boss chat panel
  terminal(title, cwd[, command])           pinned standalone terminal request
  upsertPersona(#{name,markdown,...})       id/pinnedFiles/permissions default
  setEmployeeIcon(sessionId, icon | ())
  rename(name)                              the boss's name
  renameEmployee(sessionId, name)
  regenerateAvatar([sessionId])             omit for the boss's own face
  memory(#{type,...})                       memory store op: insert/importFolder/
                                            surface/listIndex/search/readChunk/
                                            zoom — returns its filled fields
  op(#{type,...})                           escape hatch: any operation by its
                                            JSON form, result returned whole
                                            (e.g. pinDeliverable/sweepDeliverable/archiveDeliverable)
  help()                                    this text";

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
                let answer = serde_json::from_value::<BossOperation>(operation)
                    .context("not a Boss operation")
                    .and_then(|operation| {
                        if matches!(operation, BossOperation::Eval { .. }) {
                            bail!("boss eval cannot run inside an eval script");
                        }
                        dispatch(operation)
                    })
                    .and_then(|result| serde_json::to_value(&result).map_err(Into::into))
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
            .set_max_map_size(MAX_EVAL_MAP)
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
                tagged("dismissDeliverable", serde_json::json!({ "id": id.as_str() })),
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
            call(&tx, tagged("browse", serde_json::json!({ "url": url.as_str() })))
                .and_then(unwrap_result)
        }
    });
    engine.register_fn("browse", {
        let tx = tx.clone();
        move |url: ImmutableString, title: ImmutableString| -> Result<Dynamic, Box<EvalAltResult>> {
            call(&tx, tagged("browse", serde_json::json!({ "url": url.as_str(), "title": title.as_str() })))
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
            BossOperation::Roster => Ok(BossResult::Roster { roster: "1 live".into() }),
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
            error.contains("boss eval cannot run inside an eval script"),
            "unexpected error: {error}"
        );
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
