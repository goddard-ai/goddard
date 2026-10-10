//! Local, versioned Boss contracts. Operation input definitions mirror the
//! protocol's serde surface; CLI leaves retain their positional/flag grammar.
use serde_json::{Value, json};

pub(super) const FORMAT: &str = "goddard-agent.boss-contract";
pub(super) const VERSION: u32 = 1;

pub(super) fn inputs() -> Value {
    serde_json::from_str(include_str!("boss-inputs.json"))
        .expect("the checked-in Boss input definitions are valid JSON")
}

pub(super) fn availability(path: &str) -> Value {
    match path {
        "boss open" | "boss plan finalize" | "boss persona keep" | "boss persona adopt" => {
            json!({"roles":["human"],"scopedAgentCallable":false,"humanRoute":if path == "boss plan finalize" {"Approve control in the app"} else if path == "boss open" {"Open Boss in the app"} else {"Settings > Boss > Personas"},"notes":"Every scoped agent credential is rejected; listing this contract grants no authority."})
        }
        "boss report-blocker" | "steer-supervisor" | "merge submit" => {
            json!({"roles":["employee"],"scopedAgentCallable":true})
        }
        "boss summon" => {
            json!({"roles":["boss","planning","human","employee"],"conditions":["Boss experiment enabled","Employee requires summonEmployees grant and active supervisor","Employee permissions are clamped to its grants"]})
        }
        "boss prompt" | "boss stop" | "boss resume" => {
            json!({"roles":["boss","planning","human","employee-supervisor"],"conditions":["Target is an owned employee; scoped caller is active","resume requires an expired record"]})
        }
        "boss transcript" => {
            json!({"roles":["boss","planning","human","employee"],"conditions":["Employee may read itself and supervised descendants only"]})
        }
        "boss view" | "boss persona list" | "boss persona show" | "boss roster" => {
            json!({"roles":if path == "boss roster" {json!(["boss","planning","human"])} else {json!(["boss","planning","human","employee"])},"notes":"Employee views are filtered to its visible roles and employees; roster --all uses that filtered view."})
        }
        "boss file read" | "boss file list" => {
            json!({"roles":["boss","planning","human","employee"],"conditions":["Employee reads plans, pinned non-memory files, its persona, and discoverable ancestor directories; memory contents use memory operations","Paths are relative to Boss files root; traversal and symlinks rejected"]})
        }
        "boss deliverable publish" => {
            json!({"roles":["boss","planning","human","employee"],"conditions":["Employee publishes only an allowed assignment artifact path"]})
        }
        path if path.starts_with("computer ") => {
            json!({"roles":["task","employee"],"conditions":["Computer Use experiment and session enablement","Employee computerUse grant","Read bundled Computer Use skill before calling","Boss and planning principals cannot use Computer Use"]})
        }
        path if path.starts_with("boss memory ") || path.starts_with("memory ") => {
            json!({"roles":["boss","planning","human","employee","task"],"conditions":["Named bucket and operation must be granted","Project access does not grant personal bucket access","Bucket creation and migration require owner authority"]})
        }
        path if path.starts_with("boss employee ") => {
            json!({"roles":["boss","planning","human","employee-supervisor"],"conditions":["Target employee is controlled by caller","Grant increases cannot exceed employee supervisor's grants","Identity rename/icon require owner authority"]})
        }
        path if path.starts_with("boss ") => {
            json!({"roles":["boss","planning","human"],"conditions":["Boss experiment enabled","Daemon enforces operation-specific authority"]})
        }
        "create" => {
            json!({"roles":["task","human"],"conditions":["Explicit human request","Boss uses summon; employee cannot create ordinary tasks"]})
        }
        "resource acquire" | "resource run" => {
            json!({"roles":["task","employee","human"],"conditions":["Boss delegates resource execution to employees"]})
        }
        _ => {
            json!({"roles":["task","employee","boss","planning","human"],"conditions":["Scoped credential reach and operation policy apply"]})
        }
    }
}

pub(super) fn enrich_leaf(path: &str, mut leaf: Value) -> Value {
    leaf["contractFormat"] = json!("goddard-agent.command-contract");
    leaf["contractVersion"] = json!(VERSION);
    leaf["availability"] = availability(path);
    leaf["errors"] = json!({"exitStatus":2,"jsonTo":"stderr when --output json or stdout is piped","shape":{"error":{"code":"invalid_input","command":"goddard-agent","message":"string; includes daemon rejection as well as parse errors","hint":"Run the command with --help or --schema."}},"conditions":["Missing required input","Unknown or duplicate option","Malformed typed input or unsupported enum","Cross-field constraint violation","Unauthorized role or inaccessible target","Daemon unavailable or operation rejected"]});
    leaf["inputConventions"] = json!({"wholeOperationJson":false,"content":"Exactly one --text or --file; --file - reads UTF-8 stdin","structuredData":"Only explicitly documented JSON data arguments; no generic operation passthrough","helpAndSchema":"Local, read-only; no daemon or credential required"});
    if path == "boss summon" {
        let object = leaf["inputs"]
            .as_object_mut()
            .expect("leaf inputs are an object");
        object["--project"] =
            json!({"type":"absolute project path","default":"CLI current working directory"});
        for (name, definition) in [
            ("permissions", "PermissionOverrides"),
            ("resources", "ResourceSet"),
            ("new-outcome", "NewOutcome"),
        ] {
            object.insert(format!("--{name}-json|--{name}-json-file"), json!({"optional":true,"atMostOne":true,"type":"JSON object","nullable":true,"schema":{"$ref":format!("#/definitions/{definition}")},"file":"UTF-8 path; - reads stdin"}));
        }
        object.insert("--prerequisites-json|--prerequisites-json-file".into(), json!({"optional":true,"atMostOne":true,"type":"JSON UUID array","default":[],"requires":"--outcome-id","file":"UTF-8 path; - reads stdin"}));
        object.insert("--allow-burst".into(), json!({"type":"boolean flag","default":false,"notes":"May exceed liveLimit, never hardCap"}));
        object.insert("--finishes-outcome".into(), json!({"type":"boolean flag","default":false,"requires":"outcomeId or newOutcome with explicit success criteria; no other live finisher","conflictsWith":"--after-success"}));
        object.insert(
            "--group-id".into(),
            json!({"type":"string","optional":true,"notes":"Wave reports once all members finish"}),
        );
        object.insert("--priority".into(), json!({"type":"integer","minimum":i64::MIN,"maximum":i64::MAX,"optional":true,"notes":"Stored scheduling hint; FIFO ordering unchanged"}));
        object.insert(
            "--outcome-id".into(),
            json!({"type":"UUID","optional":true,"conflictsWith":"--new-outcome-json[-file]"}),
        );
        object.insert("--after-success".into(), json!({"type":"string","optional":true,"default":waku_protocol::boss::DEFAULT_AFTER_SUCCESS}));
        leaf["constraints"] = json!([
            "--item requires --plan; plan and item must be open",
            "--outcome-id and --new-outcome-json[-file] are mutually exclusive",
            "Nonempty prerequisites require --outcome-id and accepted sibling success",
            "--finishes-outcome requires outcome success criteria, a single live finisher, and no --after-success",
            "--request-id retries return the original employee; different input with the same key fails",
            "worktree requires --base-branch; adopt requires --adopt-worktree owned by a finished employee",
            "Resources must fit total host capacity; contention queues",
            "Effort must be supported by the resolved provider/model"
        ]);
        leaf["outputs"] = json!({"json":{"name":"persona name or null; not the assigned employee identity","id":"accepted employee UUID","provider":"resolved provider or null","model":"resolved model or null","effort":"resolved effort or null","workGoal":"errand|goal","project":"resolved project path","workspace":"local|worktree|adopt","state":"queued|dispatching|working","admission":"{provider,model,reasoningEffort,queuePosition,blockedBy} or null"},"notes":"Acceptance, including queued, is success; completion arrives separately."});
        leaf["definitions"] = inputs()["definitions"].clone();
    }
    if path == "boss plan finalize" {
        leaf["inputs"]["PLAN_FILE"] = json!({"positional":true,"required":true,"type":"relative plan path","notes":"Unscoped human calls must name the file; scoped agents are rejected"});
    }
    if path == "computer run" {
        leaf["constraints"] = json!([
            "url is HTTP(S), has a host, has no embedded credentials, and is at most 4096 bytes",
            "goal is nonempty and at most 4000 characters",
            "values has at most 32 distinct normalized labels, at most 32768 total bytes, labels at most 256 characters and values at most 8192 characters",
            "verify requires at least one condition; at most 16 text checks and 32 field checks; URL/text conditions are nonempty and at most 512 bytes",
            "verify field names are nonempty and at most 256 bytes; expected field values at most 8192 bytes"
        ]);
    }
    if path == "computer js" {
        leaf["inputs"]["JSON"]["maxBytes"] = json!(1048576);
    }
    leaf
}

/// Complete Boss surface, including operations deliberately owned by the client.
pub(super) fn aggregate() -> Value {
    let contract = inputs();
    let mut operations = serde_json::Map::new();
    for input in contract["definitions"]["BossOperation"]["anyOf"]
        .as_array()
        .expect("BossOperation is a tagged union")
    {
        let name = input["properties"]["type"]["const"]
            .as_str()
            .expect("operation type tag");
        let paths: &[&str] = match name {
            "view" => &[
                "boss view",
                "boss persona list",
                "boss persona show",
                "boss resource-policy show",
            ],
            "roster" => &["boss roster"],
            "context" => &["boss context"],
            "open" => &["boss open"],
            "createPlan" => &["boss plan create"],
            "browse" => &["boss browse"],
            "terminal" => &["boss terminal"],
            "finalizePlan" => &["boss plan finalize"],
            "summon" => &["boss summon"],
            "automation" => &[
                "boss automation list",
                "boss automation create",
                "boss automation update",
                "boss automation delete",
                "boss automation pause",
                "boss automation resume",
            ],
            "setResourcePolicy" => &["boss resource-policy set"],
            "updatePlanItems" => &["boss plan items"],
            "setPlanItemState" => &["boss plan item"],
            "setPlanOutcome" => &["boss plan outcome"],
            "control" => &[
                "boss prompt",
                "boss stop",
                "boss employee model",
                "boss employee permissions",
                "boss employee workspace",
                "boss employee resources",
                "boss employee plan",
                "boss employee persona",
            ],
            "resume" => &["boss resume"],
            "reportBlocker" => &["boss report-blocker"],
            "transcript" => &["boss transcript"],
            "historySearch" => &["history search"],
            "rename" => &["boss rename"],
            "renameEmployee" => &["boss employee rename"],
            "regenerateAvatar" => &["boss avatar regenerate"],
            "upsertPersona" => &["boss persona upsert"],
            "personaDefault" => &[
                "boss persona defaults",
                "boss persona reset",
                "boss persona undo",
                "boss persona keep",
                "boss persona propose",
                "boss persona adopt",
                "boss persona dismiss-proposal",
            ],
            "setEmployeeIcon" => &["boss employee icon"],
            "listFiles" => &["boss file list"],
            "readFile" => &["boss file read"],
            "writeFile" => &["boss file write"],
            "createFolder" => &["boss file mkdir"],
            "speak" => &["boss speak"],
            "publishDeliverable" => &["boss deliverable publish"],
            "dismissDeliverable" => &["boss deliverable dismiss"],
            "memory" => &[
                "boss memory buckets",
                "boss memory create",
                "boss memory overview",
                "boss memory record",
                "boss memory summary",
                "boss memory scan",
                "boss memory zoom",
                "boss memory migrate",
            ],
            "eval" => &["boss script"],
            _ => &[],
        };
        let client_owned = matches!(
            name,
            "setAvatarStyle"
                | "pinDeliverable"
                | "sweepDeliverable"
                | "archiveDeliverable"
                | "markDeliverableViewed"
                | "markGoalsViewed"
        );
        let human_only = matches!(name, "open" | "finalizePlan" | "setAvatarStyle");
        let output = match name {
            "view" | "rename" | "renameEmployee" | "regenerateAvatar" | "setAvatarStyle"
            | "upsertPersona" | "setEmployeeIcon" | "updatePlanItems" | "setPlanItemState"
            | "setPlanOutcome" | "createOutcome" | "setOutcomeState" | "resolveHandoff"
            | "setOutcomeWaiting" | "attachPlan" => "state",
            "open" | "createPlan" => "session",
            "roster" => "roster",
            "context" => "context",
            "browse" => "browse",
            "terminal" => "terminalRequested",
            "finalizePlan" => "planFinalized",
            "automation" => "automations",
            "summon" => "summoned",
            "setResourcePolicy" => "resourcePolicySet",
            "transcript" => "transcript",
            "historySearch" => "historySearch",
            "personaDefault" => "personaDefaults",
            "listFiles" => "files",
            "readFile" => "file",
            "speak" => "speak",
            "memory" => "memory",
            "eval" => "eval",
            _ => "saved",
        };
        let script = if human_only {
            Value::Null
        } else {
            json!({"command":"boss script","signature":"op(operation: map) -> BossResult map","operationType":name,"input":"Rhai map matching inputContract; use () for null","authorization":"Identical daemon checks as direct commands; script does not elevate authority"})
        };
        operations.insert(name.into(), json!({"commandPaths":paths,"inputContract":input,"actionAvailability":if name == "personaDefault" { json!({"keep":{"roles":["human"],"scopedAgentCallable":false},"adopt":{"roles":["human"],"scopedAgentCallable":false},"chooseEmployeeBase":{"roles":["human"],"scopedAgentCallable":false,"humanRoute":"Settings > Boss > Personas","cli":false}}) } else { Value::Null },"outputContract":contract["definitions"]["BossResult"]["anyOf"].as_array().expect("BossResult is tagged").iter().find(|result| result["properties"]["type"]["const"] == output).cloned().expect("operation result tag exists"),"access":{"clientOwned":client_owned,"humanOnly":human_only,"cli":!paths.is_empty(),"scripting":script},"availability":paths.first().map(|path| availability(path)).unwrap_or_else(|| if human_only {json!({"roles":["human"],"scopedAgentCallable":false,"humanRoute":"Avatar style selector in the app"})} else if client_owned {json!({"roles":["boss","planning","human"],"preferredRoute":"Client app","notes":"Client-owned protocol state; generic script op retains owner checks; no CLI leaf is invented"})} else {json!({"roles":["boss","planning","human"],"conditions":["Owner authority"]})})}));
    }
    let mut commands = super::schema()["commands"].clone();
    // All currently advertised companions are useful in the comprehensive guide;
    // their individual role gates distinguish them from Boss-owned operations.
    for (_, leaf) in commands.as_object_mut().expect("commands are a map") {
        if let Some(object) = leaf.as_object_mut() {
            object.remove("definitions");
        }
    }
    json!({"contractFormat":FORMAT,"contractVersion":VERSION,"commands":commands,"operations":operations,"definitions":contract["definitions"],"outputTypeSource":"Nested protocolType records are named protocol records in packages/waku-client/src/generated; the contract defines exact result envelopes","scripting":script_contract(),"constraints":constraint_contract(),"invocation":{"ordinary":"command paths, positionals, flags","wholeOperationJson":false,"structuredData":"Documented named JSON inputs remain valid","script":"boss script --text SOURCE or --file PATH; op maps compose operations within Rhai, not a CLI payload route"},"authorization":"Local discovery needs no daemon. Listed operations grant no authority; scoped CLI credentials cannot perform human-only reviews or impersonate a human."})
}

fn script_contract() -> Value {
    json!({"entryPoint":"boss script","scope":"fresh per invocation","examplesByOperation":script_examples(),"maxBytes":262144,"wallClockSeconds":30,"maxOperations":1000000,"imports":false,"result":"{type: eval, value: any JSON value (null for unit), output: captured print/debug text}","functions":["op(operation: map) -> tagged BossResult map","view() -> BossState map","roster() -> string","context() -> string","automation(action: map) -> AutomationsState map","summon(fields: map) -> employee UUID string","setResourcePolicy(policy: map) -> BossResourcePolicy map","control(sessionId: UUID string, action: map|string) -> unit","resume(sessionId: UUID string) -> unit","transcript(sessionId: UUID string[, turn: integer]) -> transcript map","readFile(path: string) -> {path, content}","writeFile(path: string, content: string) -> unit","listFiles([path: string]) -> file array","createFolder(path: string) -> unit","publishDeliverable(path: string[, name: string]) -> unit","dismissDeliverable(id: UUID string) -> unit","speak(parts: string|array) -> integer","browse(url: string[, title: string]) -> browse result map","terminal(title: string, cwd: string[, command: string]) -> terminal result map","upsertPersona(persona: map) -> BossState map","setEmployeeIcon(sessionId: UUID string, icon: string|unit) -> BossState map","rename(name: string) -> BossState map","renameEmployee(sessionId: UUID string, name: string) -> BossState map","regenerateAvatar([sessionId: UUID string]) -> BossState map","memory(operation: map) -> memory result map","help() -> string"],"examples":["op(#{type: \"createOutcome\", outcome: \"Ship CLI guide\", successCriteria: \"Complete offline help\"})","control(\"EMPLOYEE_UUID\", #{type: \"steer\", prompt: \"Use revised scope\", jobTitle: \"Review schema\"})"],"examplePlaceholders":"Replace EMPLOYEE_UUID with an actual employee UUID; examples are never executed by discovery."})
}

fn script_examples() -> Value {
    json!({
        "createOutcome":"op(#{type: \"createOutcome\", outcome: \"Ship CLI guide\", successCriteria: \"Complete offline guide\"})",
        "setOutcomeState":"op(#{type: \"setOutcomeState\", outcome: \"OUTCOME_UUID\", state: \"completed\", evidence: \"Guide delivered and checked\"})",
        "resolveHandoff":"op(#{type: \"resolveHandoff\", outcome: \"OUTCOME_UUID\", handoff: \"HANDOFF_UUID\", decision: #{type: \"dismiss\"}})",
        "setOutcomeWaiting":"op(#{type: \"setOutcomeWaiting\", outcome: \"OUTCOME_UUID\", waiting: #{type: \"dependency\", note: \"Awaiting review\"}, snoozedUntil: ()})",
        "attachPlan":"op(#{type: \"attachPlan\", outcome: \"OUTCOME_UUID\", plan: \"plans/cli.md\"})",
        "setProjectSubmissions":"op(#{type: \"setProjectSubmissions\", project: \"PROJECT_NAME\", enabled: true})",
        "setProjectQaBranch":"op(#{type: \"setProjectQaBranch\", project: \"PROJECT_NAME\", branch: \"dev\"})",
        "placeholders":"Replace UUID/project/plan placeholders with actual accessible records before execution. Examples mutate only when explicitly invoked."
    })
}

fn constraint_contract() -> Value {
    json!({"summon":["outcomeId and newOutcome are mutually exclusive","finishesOutcome requires an outcome with explicit success criteria and no other live finisher; conflicts with afterSuccess","prerequisites require existing outcomeId and sibling assignment UUIDs","item requires plan; unknown/closed outcome or plan and done/dropped items reject","requestId retries must carry identical fields","worktree requires baseBranch; adopt requires finished-owner adoptWorktree","PermissionOverrides inherit per field and clamp employee supervisor grants","resources must fit host capacity; contested resources queue","priority is stored but FIFO is unchanged"],"setOutcomeState":["completed requires nonempty evidence and no pending handoffs","cancelled stops live assignments; open reopens"],"resolveHandoff":["Handoff must be pending on outcome","assign references an existing follow-up assignment","completeOutcome requires evidence; other pending handoffs block completion"],"attachPlan":["Plan must be approved; attaching does not start work or change outcome state"],"setOutcomeWaiting":["waiting=null clears wait; snoozedUntil=null clears snooze","until.at and snoozedUntil are Unix timestamps; dependency waits persist until resolved/cleared"],"control":["setPlan fields are tri-state: omitted keeps, null clears; changing plan without item drops allocation","setWorkspace allows local or worktree, never adopt; worktree requires baseBranch","setModel provider/model must exist in catalog; omitted effort resolves target default","setPermissions updates per field; cannot exceed supervisor grants","steer supports optional jobTitle; CLI boss prompt --delivery steer cannot retitle, use script control"],"personaDefault":["keep, adopt and chooseEmployeeBase require human client; scoped agents are rejected","Choose base in Settings > Boss > Personas; no CLI command is invented"],"finalizePlan":["Human client only; all scoped agent calls rejected without creating an approval request","Unscoped human call requires planFile; approval freezes document"],"setResourcePolicy":["expectedRevision must match; liveLimit >= 0; hardCap >= liveLimit","Lowering caps does not stop active work"],"files":["Relative paths only; traversal and symlinks rejected","Finalized plan documents are frozen; persona Markdown uses persona upsert"],"automation":["update requires existing id; create ignores supplied id","Workspace worktree requires baseBranch; existing requires sessionId","Schedule fields must be in range; scheduled/webhook configuration is validated by daemon"]})
}
