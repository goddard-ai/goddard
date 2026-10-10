//! Local readable discovery, derived from the same leaf contracts as schemas.
use serde_json::{Value, json};

const ORIENTATION: &str = r#"Use command paths, positionals, and flags. JSON is only for documented structured input data
and output. This guide is local and read-only; listing a capability never grants authorization.
Full contracts: goddard-agent boss --schema. Focus a family with boss employee --help, or a
command with boss summon --help. Help stays complete when piped; --output json explicitly
selects the structured readable guide. UUID, PATH, NAME and other uppercase examples are
templates: substitute your values. Examples are never executed. Text inputs use exactly one of
--text TEXT or --file PATH; --file - reads stdin where documented. Operation output defaults to
text on terminals and JSON in pipes; --output text|json overrides it."#;

const WORKFLOW: &str = r#"Summon starts a new job; accepted/queued is not finished and is not a failure. Do not summon
duplicates while waiting. Prompt defaults to interrupt: steer a live turn, otherwise queue;
queue waits behind current work; steer requires a live turn. Boss policy requires an explicit
employee icon even though the CLI permits omission. Respect assigned model routing, resource
caps and human preferences. Employees own execution, watching, commits and merge submit; Boss
delegates those jobs. errand finishes report upward; goal completion is visible in Goals
without a routine success report; actionable blockers still report upward. Employees cannot prompt or steer supervisors; results are delivered at turn end. Persona names resolve exactly;
use UUIDs if ambiguous. Project selection does not grant private memory access. Plan
finalization freezes the human-approved document, never replaces human review. Commit project
documentation; publish Boss-facing reports as deliverables. On errors, correct the named input,
consult that command's --help or --schema, and retry only when safe; do not automatically
replay a mutation after losing its response."#;

const SCRIPTING: &str = r#"Boss-only composition: goddard-agent boss script (--text SOURCE | --file PATH|-). Rhai scopes are fresh per call; variables are not durable. Direct commands are preferred when available. In Rhai, #{...} is a map, () is null, and UUIDs are strings. Each binding performs the corresponding authorized operation and returns its result (Saved returns ()).

Bindings with direct-command counterparts:
  view(), roster(), context(), resume(sessionId), transcript(sessionId[, turn])
  summon(#{jobTitle,prompt,project,personaId?,icon?,provider?,model?,reasoningEffort?,workspace?,baseBranch?,adoptWorktree?,workGoal?,permissions?,resources?,allowBurst?,groupId?,priority?,outcomeId?,newOutcome?,afterSuccess?,finishesOutcome?,prerequisites?,plan?,item?,requestId?})
    workspace defaults local; worktree requires baseBranch; adopt requires adoptWorktree. outcomeId or newOutcome assigns an outcome; finishesOutcome requires success criteria. prerequisites are sibling employee UUIDs; groupId groups a completion wave. permissions overrides per-field inherited grants (bucketIds, integrationIds, summonEmployees, computerUse); supervisor grants clamp delegation. resources declares host needs, allowBurst requests capacity within hard caps; priority is recorded; FIFO admission does not reorder on it. afterSuccess is durable follow-up intent.
  setResourcePolicy(#{expectedRevision,modelLimits,host?}) — update against the current revision; modelLimits contains provider/model/liveLimit/hardCap, and host contains capacity counts.
  control(sessionId, "stop" | #{type:ACTION,...})
    ACTION: prompt (prompt, delivery?=interrupt), steer (prompt, jobTitle?), stop, setModel (provider, model, reasoningEffort?), setPermissions (permissions), setPersona (personaId or ()), setWorkspace (workspace: local|worktree, baseBranch?), setResources (resources), setPlan (plan?, item?; omitted keeps, () clears).
  readFile(path), writeFile(path, content), listFiles([path]), createFolder(path)
  publishDeliverable(path[, name]), dismissDeliverable(id)
  speak(parts | "utterance"), browse(url[, title]), terminal(title,cwd[,command])
  upsertPersona(#{name,markdown,id?,pinnedFiles?,permissions?,icon?})
  setEmployeeIcon(sessionId, icon | ()), rename(name), renameEmployee(sessionId,name), regenerateAvatar([sessionId])
  memory(#{type:TYPE,bucket,...}) — listBuckets/createBucket/overview/record/submitSummary/scan/zoomBucket; content operations name the bucket.
  automation(#{type:TYPE,...}) — list/create/update/delete/pause/resume
  help() — runtime binding help

Operations without a direct CLI leaf appear in the operation cards below. Invoke an agent-callable operation through op(#{type:"OPERATION",...}) within boss script --text SOURCE or --file PATH. Human-review-only calls are performed in the client, and client-owned sidebar state prefers the client controls; scripting never elevates authority.

Example (creates an outcome): goddard-agent boss script --text 'op(#{type:"createOutcome", outcome:"CLI discovery", successCriteria:"Complete local guide"})'
Example (read-only): goddard-agent boss script --text 'let s = view(); print(roster()); s'"#;

fn group(path: &str) -> &str {
    if !path.starts_with("boss ") {
        return "Companion operations";
    }
    match path.split_whitespace().nth(1).unwrap_or("") {
        "context" | "view" | "roster" | "transcript" | "report-blocker" => {
            "Understand current work"
        }
        "summon" | "prompt" | "stop" | "resume" | "employee" => "Delegate and manage employees",
        "persona" => "Maintain roles",
        "file" | "memory" => "Maintain persistent knowledge",
        "plan" => "Plan and track outcomes",
        "deliverable" | "speak" | "browse" | "open" | "terminal" => "Publish and communicate",
        "automation" | "resource-policy" => "Schedule and allocate capacity",
        _ => "Customize and compose",
    }
}

fn authority(path: &str) -> &str {
    match path {
        "steer-supervisor" | "merge submit" | "boss report-blocker" => {
            "Employee-only; assigned scope and grants apply"
        }
        "boss persona keep" | "boss persona adopt" => {
            "Human-review-only; agents are rejected. Review in Settings → Boss → Personas"
        }
        "boss open" => {
            "Human-only; agent credentials are rejected. Enable the Boss experiment and open Boss chat in the client"
        }
        "boss file read"
        | "boss file list"
        | "boss persona list"
        | "boss persona show"
        | "boss persona defaults"
        | "boss deliverable publish" => {
            "Context-dependent; employees need granted knowledge or their own artifact access"
        }
        p if p.starts_with("boss memory ") || p.starts_with("memory ") => {
            "Context-dependent; only granted buckets; create/migrate are Boss-only"
        }
        p if p.starts_with("computer ") => {
            "Context-dependent; Computer Use must be enabled and granted; read the bundled Computer Use skill first; existing approval prompts apply"
        }
        "create" | "rename" => {
            "Ordinary-task-only; Boss credentials are rejected; rename may wait for human approval"
        }
        "command list" | "command upsert" | "command remove" => {
            "Settings-write capability required; Boss/planning sessions are rejected"
        }
        "resource acquire" | "resource run" => {
            "Ordinary tasks and employees only; Boss credentials cannot acquire host resources"
        }
        "prompt" => {
            "Context-dependent; only on the human's explicit request; employees message only their supervisor"
        }
        "archive" | "ask" => {
            "Context-dependent; waits for the human decision where required; cannot bypass approval"
        }
        p if p.starts_with("boss ") => {
            "Boss-only unless the daemon grants supervised employee access; persona policy still controls"
        }
        _ => "Context-dependent; session credential, project scope and workflow policy apply",
    }
}

fn purpose(path: &str) -> &str {
    match path {
        "boss summon" => "Assign a new employee job; acceptance can queue before execution.",
        "boss prompt" | "prompt" => "Send work or a correction with explicit delivery semantics.",
        "boss transcript" | "read" => "Read retained transcript turns without starting work.",
        "boss roster" => "Inspect employee status; --all includes finished employees.",
        "boss view" => "Inspect full Boss state, including employees, roles and outcomes.",
        "boss context" => "Read a bounded digest of current projects, tasks and schedules.",
        "boss stop" => "Stop an employee's active work.",
        "boss resume" => "Revive an expired employee in place, retaining transcript and workspace.",
        "boss script" => "Compose authorized Boss operations in a fresh Rhai scope.",
        "boss report-blocker" => "Interrupt your supervisor only when external action is needed.",
        "boss plan finalize" => "Freeze the human-approved plan and create its tracked work items.",
        "boss deliverable publish" => {
            "Store a durable Boss-facing artifact or reference for review."
        }
        "boss open" => "Human-only: open or configure the Boss session.",
        "create" => "Create a separate task only when the human requests it.",
        "search" => "Search task transcripts in the credential's project scope.",
        "history search" => "Search retained tasks, employees and Boss chats, including archives.",
        "map" => "Retrieve ranked code context; read returned source before editing.",
        "models" => "Discover supported provider/model combinations and efforts.",
        "merge submit" => "Submit this employee's committed worktree to the project's QA branch.",
        "steer-supervisor" => {
            "Unavailable: employees report actionable blockers or results at turn end."
        }
        "rename" => "Request a new title for this task after a substantial change in purpose.",
        "archive" => "Propose archiving named tasks; waits for human approval.",
        "ask" => "Ask structured questions and wait for the human's answer.",
        "boss employee rename" => {
            "Change an employee's display name without replacing its assignment."
        }
        "boss employee icon" => "Set or clear an employee's work icon.",
        "boss employee model" => {
            "Atomically switch model and resume the assignment; provider/model changes re-enter admission."
        }
        "boss employee permissions" => {
            "Replace individual grants; memory/delegation apply now, integration/Computer Use on next launch."
        }
        "boss employee persona" => {
            "Select an employee role or clear it to the canonical Employee base."
        }
        "boss employee plan" => {
            "Retag plan/item links; omitted fields keep, null clears, changing plan alone drops item."
        }
        "boss employee workspace" => {
            "Interrupt, rebind the workspace and resume; failure preserves the previous workspace."
        }
        "boss employee resources" => {
            "Re-admit queued work or wait to swap running capacity without stopping it."
        }
        "boss persona list" => "List the roles visible to this credential.",
        "boss persona show" => {
            "Read one exact role name or UUID; use UUID when names are ambiguous."
        }
        "boss persona upsert" => {
            "Create or replace role instructions with explicit new/id selection."
        }
        "boss persona defaults" => {
            "Inspect shipped revisions, saved defaults and pending proposals."
        }
        "boss persona reset" => {
            "Reset a role to the latest shipped instructions, retaining an undo record."
        }
        "boss persona undo" => {
            "Restore a reset/adoption only while the saved instructions still match its result."
        }
        "boss persona keep" => {
            "Human-only: acknowledge a shipped update while keeping customized instructions."
        }
        "boss persona propose" => {
            "Save proposed role instructions for human review; does not adopt them."
        }
        "boss persona adopt" => "Human-only: adopt reviewed role instructions.",
        "boss persona dismiss-proposal" => "Discard the pending instruction proposal.",
        "boss file list" => "List a Boss knowledge directory; omitted path selects the root.",
        "boss file read" => "Read a granted Boss knowledge file by relative path.",
        "boss file write" => "Replace Boss knowledge text; frozen approved plans reject writes.",
        "boss file mkdir" => "Create a relative directory in Boss knowledge.",
        "boss plan create" => {
            "Open a dedicated planning session; omitted model selection uses planning defaults."
        }
        "boss plan items" => {
            "Replace the live ordered work-item list without rewriting the frozen plan."
        }
        "boss plan item" => "Mark a tracked item toDo, done or dropped.",
        "boss plan outcome" => {
            "Record a plan's approved, completed or abandoned state, separately from outcome completion."
        }
        "boss automation list" => "Inspect schedules and run history.",
        "boss automation create" => "Create a scheduled/webhook job; enabled defaults false.",
        "boss automation update" => "Replace an identified scheduled job's definition.",
        "boss automation delete" => "Delete a scheduled job by UUID.",
        "boss automation pause" => "Disable future scheduled runs for a job.",
        "boss automation resume" => "Enable future scheduled runs for a job.",
        "boss resource-policy show" => {
            "Inspect capacity limits and the revision needed to update them."
        }
        "boss resource-policy set" => {
            "Replace capacity policy against its expected revision; hard caps remain controlling."
        }
        "resource acquire" => {
            "Reserve host capacity under the owning shell; waits up to the requested bound."
        }
        "resource run" => {
            "Reserve capacity, supervise a command, and release on completion; nested calls reuse subsets."
        }
        "resource release" => {
            "Release your reservation; live workloads/devices retain capacity until stopped."
        }
        "resource cancel" => "Cancel your queued request or stop your supervised workload.",
        "resource status" => "Inspect shared owners, queues, capacity and observation failures.",
        "computer js" => "Execute raw JavaScript in this task's persistent kernel.",
        "computer run" => {
            "Run a bounded browser workflow with explicit URL, goal and supplied completion checks."
        }
        "computer reset" => "Reset this task's Computer Use kernel and its retained bindings.",
        "command list" => "List the user's saved commands.",
        "command upsert" => "Save a structured shell-command definition; script must be nonempty.",
        "command remove" => "Remove one saved command by UUID or exact name.",
        p if p.starts_with("boss employee ") => {
            "Update the employee's named setting; active work may be reconfigured."
        }
        p if p.starts_with("boss persona ") => {
            "Inspect or maintain role instructions and default-review state."
        }
        p if p.contains(" memory ") || p.starts_with("memory ") => {
            "Read or maintain durable notes in an explicitly accessible bucket."
        }
        p if p.starts_with("boss file ") => "Read or maintain persistent Boss knowledge files.",
        p if p.starts_with("boss plan ") => {
            "Create a planning session or update tracked plan progress."
        }
        p if p.starts_with("boss automation ") => {
            "Inspect or maintain scheduled/webhook work and its run history."
        }
        p if p.starts_with("boss resource-policy ") => {
            "Inspect or revise model and host capacity limits."
        }
        p if p.starts_with("resource ") => {
            "Inspect or reserve shared host capacity for supervised work."
        }
        p if p.starts_with("computer ") => "Operate this task's enabled Computer Use runtime.",
        p if p.starts_with("command ") => "Manage the user's saved shell commands.",
        "boss deliverable dismiss" => "Dismiss an artifact from active review.",
        "boss speak" => "Speak an utterance to connected clients.",
        "boss browse" => "Open an HTTP(S) page in the Boss chat panel.",
        "boss terminal" => "Request a pinned standalone terminal with an optional startup script.",
        "boss rename" => "Change the Boss display name.",
        "boss avatar regenerate" => "Regenerate the Boss or named employee avatar.",
        _ => "Use this command's documented inputs and observe its returned result.",
    }
}

// Render contract fields as prose, with full nested coverage but no JSON dump.
fn describe(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) if items.is_empty() => "[]".to_owned(),
        Value::Array(items) if items.len() > 12 => {
            format!("{} values; complete enum in --schema", items.len())
        }
        Value::Array(items) => items.iter().map(describe).collect::<Vec<_>>().join(" | "),
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| match (key.as_str(), value) {
                ("required", Value::Bool(true)) => "required".to_owned(),
                ("optional", Value::Bool(true)) | ("required", Value::Bool(false)) => {
                    "optional".to_owned()
                }
                ("exactlyOne", Value::Bool(true)) => "choose exactly one".to_owned(),
                ("positional", Value::Bool(true)) => "positional".to_owned(),
                ("requiredWhen", value) => format!("required when {}", describe(value)),
                ("requiredUnless", value) => format!("required unless {}", describe(value)),
                ("enum", value) => format!("one of {}", describe(value)),
                _ => format!("{key}: {}", describe(value)),
            })
            .collect::<Vec<_>>()
            .join("; "),
        other => other.to_string(),
    }
}

fn schema_summary(schema: &Value, definitions: &Value, depth: usize) -> String {
    if let Some(reference) = schema["$ref"].as_str() {
        let name = reference.rsplit('/').next().unwrap_or(reference);
        return if depth < 4 {
            format!(
                "{name}: {}",
                schema_summary(&definitions[name], definitions, depth + 1)
            )
        } else {
            name.to_owned()
        };
    }
    if let Some(variants) = schema["anyOf"].as_array() {
        return variants
            .iter()
            .map(|variant| schema_summary(variant, definitions, depth + 1))
            .collect::<Vec<_>>()
            .join(" or ");
    }
    if let Some(value) = schema.get("const") {
        return describe(value);
    }
    if let Some(values) = schema.get("enum") {
        return describe(values);
    }
    let mut summary = match schema["type"].as_str() {
        Some("object") => {
            let required = schema["required"].as_array();
            let fields = schema["properties"]
                .as_object()
                .map(|fields| {
                    fields
                        .iter()
                        .map(|(name, field)| {
                            format!(
                                "{name} {} ({})",
                                schema_summary(field, definitions, depth + 1),
                                if required
                                    .is_some_and(|items| items.iter().any(|item| item == name))
                                {
                                    "required"
                                } else {
                                    "optional"
                                }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default();
            format!("object {{{fields}}}")
        }
        Some("array") => format!(
            "array of {}",
            schema_summary(&schema["items"], definitions, depth + 1)
        ),
        Some(kind) => kind.to_owned(),
        None => "structured data".to_owned(),
    };
    for key in [
        "default",
        "minimum",
        "maximum",
        "minLength",
        "maxLength",
        "additionalProperties",
    ] {
        if let Some(value) = schema.get(key) {
            summary.push_str(&format!("; {key}: {}", describe(value)));
        }
    }
    summary
}

fn input_description(contract: &Value, definitions: &Value) -> String {
    let mut contract = contract.clone();
    if let Some(schema) = contract.get("schema").cloned() {
        contract["schema"] = json!(schema_summary(&schema, definitions, 0));
    }
    describe(&contract)
}

fn operation_purpose(name: &str) -> &str {
    match name {
        "createOutcome" => {
            "Create a desired result with success criteria; starts without assignments."
        }
        "setOutcomeState" => {
            "Complete with evidence, cancel live assignments, or reopen an outcome."
        }
        "resolveHandoff" => {
            "Decide whether an assignment result needs follow-up, dismissal or outcome completion."
        }
        "setOutcomeWaiting" => "Record a deliberate wait or reminder snooze; null clears it.",
        "attachPlan" => "Attach an approved approach without starting or completing work.",
        "setProjectSubmissions" => {
            "Enable or disable employee worktree submissions for a registered project."
        }
        "setProjectQaBranch" => {
            "Override the checked-out QA branch for submissions and review; null restores the global default."
        }
        "setAvatarStyle" => "Human-only: choose one avatar generator for Boss and all employees.",
        "pinDeliverable" => "Pin or unpin a deliverable in the sidebar.",
        "sweepDeliverable" => "Move a deliverable into or out of the dormant sidebar fold.",
        "archiveDeliverable" => "Archive or restore a deliverable without deleting its record.",
        "markDeliverableViewed" => {
            "Record that a deliverable was opened and retire its unread marker."
        }
        "markGoalsViewed" => "Record opening Goals and clear earlier completion unread markers.",
        _ => "Use the typed operation through its declared access route.",
    }
}

fn operation_cards(aggregate: &Value, family: &str) -> Vec<Value> {
    if !matches!(family, "boss" | "boss script") {
        return Vec::new();
    }
    aggregate["operations"].as_object().into_iter().flatten().filter(|(_, operation)| operation["commandPaths"].as_array().is_some_and(Vec::is_empty)).map(|(name, operation)| {
        let contract = &operation["inputContract"];
        let required = contract["required"].as_array();
        let inputs: Vec<_> = contract["properties"].as_object().into_iter().flatten().filter(|(field, _)| *field != "type").map(|(field, value)| json!({"name":field,"description":format!("{}; {}", if required.is_some_and(|fields| fields.iter().any(|item| item == field)) { "required" } else { "optional" }, schema_summary(value, &aggregate["definitions"], 0))})).collect();
        json!({"operation":name,"purpose":operation_purpose(name),"route":if operation["access"]["humanOnly"] == true {"Human client only; scoped CLI/script calls are rejected"} else if operation["access"]["clientOwned"] == true {"Client-owned sidebar state; prefer the app. No direct CLI leaf; authorized Boss script op retains owner checks"} else {"Scripting-only: boss script --text SOURCE or --file PATH; op(#{type:\"OPERATION\",...})"},"availability":describe(&operation["availability"]),"inputs":inputs,"constraints":aggregate["constraints"][name],"outcome":schema_summary(&operation["outputContract"], &aggregate["definitions"], 0),"example":aggregate["scripting"]["examplesByOperation"][name]})
    }).collect()
}

fn syntax(path: &str, inputs: &Value) -> String {
    let mut usage = format!("goddard-agent {path}");
    if let Some(fields) = inputs.as_object() {
        for (name, contract) in fields {
            if !name.starts_with("--") && contract.get("positional") != Some(&Value::Bool(true)) {
                continue;
            }
            let required = contract.get("required") == Some(&Value::Bool(true));
            let item = if name == "--" {
                "-- COMMAND [ARGS]".to_owned()
            } else if name.starts_with("--") {
                if contract == "include finished employees"
                    || matches!(
                        contract.get("type").and_then(Value::as_str),
                        Some("boolean" | "boolean flag")
                    )
                    || matches!(name.as_str(), "--all" | "--new" | "--clear" | "--reference")
                {
                    name.clone()
                } else {
                    name.split('|')
                        .map(|flag| format!("{flag} VALUE"))
                        .collect::<Vec<_>>()
                        .join(" | ")
                }
            } else {
                name.clone()
            };
            usage.push_str(&format!(
                " {}",
                if required {
                    if item.contains(" | ") {
                        format!("({item})")
                    } else {
                        item
                    }
                } else {
                    format!("[{item}]")
                }
            ));
        }
    }
    usage
}

pub(super) fn guide(schema: &Value, family: &str) -> Value {
    let aggregate = super::boss_contract::aggregate();
    let commands = schema["commands"]
        .as_object()
        .expect("local command contracts");
    let cards: Vec<_> = commands.iter().filter(|(path, _)| family == "boss" || *path == family || path.starts_with(&format!("{family} "))).map(|(path, leaf)| {
        let inputs: Vec<_> = leaf["inputs"].as_object().into_iter().flatten().map(|(name, contract)| json!({"name":name,"description":input_description(contract, &aggregate["definitions"])})).collect();
        let availability = if path.starts_with("boss ") || path.starts_with("computer ") || matches!(path.as_str(), "prompt" | "steer-supervisor") { describe(&leaf["availability"]) } else { authority(path).to_owned() };
        json!({"command":path,"group":group(path),"purpose":purpose(path),"syntax":syntax(path, &leaf["inputs"]),"availability":availability,"inputs":inputs,"constraints":leaf["constraints"],"outcome":describe(&leaf["outputs"]),"example":if path == "boss summon" { json!("goddard-agent boss summon --title 'Audit CLI help' --icon search --text 'Research only; report findings.'") } else { leaf["example"].clone() },"schema":format!("goddard-agent {path} --schema")})
    }).collect();
    json!({"title":format!("goddard-agent {family} — capability and usage guide"),"orientation":ORIENTATION,"cards":cards,"operationCards":operation_cards(&aggregate, family),"humanReviewActions":aggregate["operations"]["personaDefault"]["actionAvailability"],"scripting":if family == "boss" || family == "boss script" { SCRIPTING } else { "" },"workflow":WORKFLOW})
}

pub(super) fn render(guide: &Value) -> String {
    let mut text = format!(
        "{}\n\n{}\n\nCOMMAND INDEX\n",
        guide["title"].as_str().unwrap_or(""),
        guide["orientation"].as_str().unwrap_or("")
    );
    let cards = guide["cards"].as_array().expect("guide cards");
    let groups = [
        "Understand current work",
        "Delegate and manage employees",
        "Maintain roles",
        "Maintain persistent knowledge",
        "Plan and track outcomes",
        "Publish and communicate",
        "Schedule and allocate capacity",
        "Customize and compose",
        "Companion operations",
    ];
    for group in groups {
        let selected: Vec<_> = cards.iter().filter(|card| card["group"] == group).collect();
        if selected.is_empty() {
            continue;
        }
        text.push_str(&format!("\n{group}\n"));
        for card in &selected {
            text.push_str(&format!(
                "  {} — {}\n",
                card["command"].as_str().unwrap_or(""),
                card["purpose"].as_str().unwrap_or("")
            ));
        }
    }
    if let Some(cards) = guide["operationCards"]
        .as_array()
        .filter(|cards| !cards.is_empty())
    {
        text.push_str("\nScripting and client operations (no direct CLI leaf)\n");
        for card in cards {
            text.push_str(&format!(
                "  {} — {}\n",
                card["operation"].as_str().unwrap(),
                card["purpose"].as_str().unwrap()
            ));
        }
    }
    text.push_str("\nUSAGE CARDS\n");
    for group in groups {
        for card in cards.iter().filter(|card| card["group"] == group) {
            text.push_str(&format!(
                "\n{} — {}\n  Usage: {}\n  Authority: {}\n",
                card["command"].as_str().unwrap_or(""),
                card["purpose"].as_str().unwrap_or(""),
                card["syntax"].as_str().unwrap_or(""),
                card["availability"].as_str().unwrap_or("")
            ));
            for input in card["inputs"].as_array().unwrap() {
                text.push_str(&format!(
                    "  {}: {}\n",
                    input["name"].as_str().unwrap(),
                    input["description"].as_str().unwrap()
                ));
            }
            if let Some(constraints) = card["constraints"].as_array() {
                for constraint in constraints {
                    text.push_str(&format!("  Constraint: {}\n", describe(constraint)));
                }
            }
            text.push_str(&format!(
                "  Outcome: {}\n  Example: {}\n  Schema: {}\n",
                card["outcome"].as_str().unwrap_or(""),
                card["example"].as_str().unwrap_or(""),
                card["schema"].as_str().unwrap_or("")
            ));
        }
    }
    if let Some(scripting) = guide["scripting"].as_str().filter(|s| !s.is_empty()) {
        text.push_str(&format!("\nSCRIPTING ROUTES\n{scripting}\n"));
    }
    if let Some(cards) = guide["operationCards"]
        .as_array()
        .filter(|cards| !cards.is_empty())
    {
        text.push_str("\nOPERATIONS WITHOUT DIRECT CLI LEAVES\n");
        for card in cards {
            text.push_str(&format!(
                "\n{} — {}\n  Route: {}\n  Authority: {}\n",
                card["operation"].as_str().unwrap(),
                card["purpose"].as_str().unwrap(),
                card["route"].as_str().unwrap(),
                card["availability"].as_str().unwrap()
            ));
            for input in card["inputs"].as_array().unwrap() {
                text.push_str(&format!(
                    "  {}: {}\n",
                    input["name"].as_str().unwrap(),
                    input["description"].as_str().unwrap()
                ));
            }
            if let Some(constraints) = card["constraints"].as_array() {
                for constraint in constraints {
                    text.push_str(&format!("  Constraint: {}\n", describe(constraint)));
                }
            }
            text.push_str(&format!(
                "  Outcome: {}\n",
                card["outcome"].as_str().unwrap()
            ));
            if let Some(example) = card["example"].as_str() {
                text.push_str(&format!(
                    "  Example source (mutates when invoked): {example}\n"
                ));
            }
            text.push_str("  Schema: goddard-agent boss --schema\n");
        }
    }
    if let Some(actions) = guide["humanReviewActions"].as_object() {
        text.push_str("\nHUMAN PERSONA REVIEW\n");
        for (name, availability) in actions {
            text.push_str(&format!(
                "  personaDefault {name}: {}. Review in Settings → Boss → Personas.\n",
                describe(availability)
            ));
        }
        text.push_str("  chooseEmployeeBase selects a saved non-Boss, non-specialist persona by personaId UUID; it has no CLI leaf. Agent scripts cannot adopt, keep or choose a base.\n");
    }
    text.push_str(&format!(
        "\nWORKFLOW AND RECOVERY\n{}\n",
        guide["workflow"].as_str().unwrap_or("")
    ));
    text
}
