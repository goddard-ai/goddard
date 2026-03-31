# Workforce Human Testing Guide

This guide is for a human operator who wants to exercise the current workforce feature end to end.

Workforce is still pre-alpha and headless. The supported operator surface today is the daemon plus the `goddard-workforce` CLI. There is not yet a dedicated app UI.

## What You Are Testing

Workforce gives one repository:

- one root agent for repo-wide coordination
- zero or more domain agents for narrower owned paths
- one daemon-owned runtime that queues, routes, and replays work

The durable state lives in:

- `.goddard/workforce.json`
- `.goddard/ledger.jsonl`

For a first test pass, focus on routing, ownership, delegation, suspension, and observability. Do not treat the current feature as production-safe shared-tree automation yet. Git enforcement and response validation are still incomplete.

## What To Expect Today

- The runtime is daemon-owned, not client-owned.
- Requests are handled sequentially per agent.
- The root agent owns the whole repository.
- Domain agents own narrow relative paths.
- The CLI can initialize, start, inspect, request, update, cancel, truncate, and stop workforce.
- The best current observation surfaces are daemon logs, `goddard-workforce status`, and `.goddard/ledger.jsonl`.
- You will not get a polished chat transcript viewer for inter-agent traffic.

## Preconditions

- You are inside a git repository.
- The daemon can run locally.
- The workforce and daemon packages have been built if you want to invoke their local binaries directly.

Examples in this guide assume you are at the repository root.

If `goddard-daemon` and `goddard-workforce` are already on `PATH`, use them directly.

If they are not on `PATH`, build first:

```bash
bun --filter @goddard-ai/daemon run build
bun --filter @goddard-ai/workforce run build
```

Then use the local entrypoints:

```bash
node ./core/daemon/dist/main.mjs run --verbose
node ./workforce/dist/main.js --help
```

## Recommended Terminal Layout

Use three terminals.

- Terminal 1: run the daemon with verbose logging
- Terminal 2: run `goddard-workforce` commands
- Terminal 3: watch the ledger file

Example:

```bash
# terminal 1
node ./core/daemon/dist/main.mjs run --verbose
```

```bash
# terminal 2
node ./workforce/dist/main.js --help
```

```bash
# terminal 3
tail -f .goddard/ledger.jsonl
```

If you already have the package binaries on `PATH`, replace those `node .../dist/...` commands with `goddard-daemon` and `goddard-workforce`.

## First-Time Setup

Initialize workforce for the repository:

```bash
node ./workforce/dist/main.js init --root "$PWD"
```

The initializer discovers package roots and prompts you to choose which ones become workforce domains.

After init, inspect `.goddard/workforce.json` carefully.

Confirm:

- `version` is `1`
- `rootAgentId` points at a real agent, usually `root`
- the root agent has `cwd: "."` and `owns: ["."]`
- each domain agent has a stable id, a narrow `cwd`, and narrow `owns`
- there are no accidental overlaps unless you intentionally want them

## Start And Inspect The Runtime

Start workforce:

```bash
node ./workforce/dist/main.js start --root "$PWD"
```

Inspect it:

```bash
node ./workforce/dist/main.js status --root "$PWD"
node ./workforce/dist/main.js list
```

The current status output is useful but shallow. Treat it as a runtime summary, not a transcript.

## Recommended Test Passes

Run these in order.

### 1. Happy-Path Root Request

Queue one simple read-only request to the root agent:

```bash
node ./workforce/dist/main.js request --root "$PWD" --target-agent-id root \
  "Inspect the repository, do not change files, and respond with a brief summary of the active workforce topology."
```

What to look for:

- a new `request` event in `.goddard/ledger.jsonl`
- a `handle` event when the daemon starts work
- a `response` or `suspend` event afterward
- daemon log lines showing request id, agent id, and session launch/completion

### 2. Direct Domain Request

Pick one real domain agent id from `.goddard/workforce.json` and queue a direct request:

```bash
node ./workforce/dist/main.js request --root "$PWD" --target-agent-id <agent-id> \
  "Inspect only your owned paths, do not change files, and respond with what you believe you own."
```

This validates:

- domain routing
- owned-path guidance
- per-agent sequential handling

### 3. Root Delegation

Ask the root agent to delegate a small read-only task:

```bash
node ./workforce/dist/main.js request --root "$PWD" --target-agent-id root \
  "Delegate one read-only check to the most appropriate domain agent, then respond with what was delegated and what came back."
```

This is the most important workflow to validate because it exercises root coordination instead of only direct operator-to-agent routing.

### 4. Suspension And Resume

Queue a request that should suspend:

```bash
node ./workforce/dist/main.js request --root "$PWD" --target-agent-id root \
  "If you are missing context or authority, suspend with a clear reason instead of guessing."
```

Then resume it with an update:

```bash
node ./workforce/dist/main.js update --root "$PWD" --request-id <request-id> \
  "Here is the missing context you asked for."
```

Important current behavior:

- `update` appends more input
- if the request was suspended, it resumes by requeueing
- if the request was active, the current implementation also requeues it rather than steering the live session in place

### 5. Cancel And Truncate

Cancel one request:

```bash
node ./workforce/dist/main.js cancel --root "$PWD" --request-id <request-id> --reason \
  "Operator cancelled test request."
```

Truncate one agent queue or the whole workforce:

```bash
node ./workforce/dist/main.js truncate --root "$PWD" --agent-id <agent-id> --reason \
  "Operator clearing pending test work."
```

Use this to confirm operator control paths and ledger recording.

### 6. Restart And Replay

Stop workforce:

```bash
node ./workforce/dist/main.js stop --root "$PWD"
```

Start it again:

```bash
node ./workforce/dist/main.js start --root "$PWD"
node ./workforce/dist/main.js status --root "$PWD"
```

Confirm that queued or suspended work is still reconstructed from `.goddard/ledger.jsonl`.

## What The Operator CLI Accepts

The operator-facing `goddard-workforce` CLI accepts inline prompt text for:

- `request`
- `create`
- `update`

You can pass the message with `--message` or positionally.

Examples:

```bash
node ./workforce/dist/main.js request --root "$PWD" --target-agent-id root \
  "Review the current queue."
```

```bash
node ./workforce/dist/main.js create --root "$PWD" \
  "Add a new package only if the feature truly requires one."
```

Inside an active agent session, the injected `workforce` command is different. It uses file-backed payloads such as:

- `workforce request --target-agent-id <agent-id> --input-file <path>`
- `workforce update --request-id <request-id> --input-file <path>`
- `workforce respond --output-file <path>`
- `workforce suspend --reason-file <path>`

That distinction matters when you read the daemon prompts or watch agent behavior.

## What You Can Observe

You can see these things today.

### Status Summary

`goddard-workforce status` shows:

- runtime state
- config path
- ledger path
- counts for queued, active, suspended, and failed work
- active workforce config

It does not show a full chat transcript.

### Ledger History

`.goddard/ledger.jsonl` is the best durable record of request flow.

You should expect to see events like:

- `request`
- `handle`
- `response`
- `suspend`
- `update`
- `cancel`
- `truncate`
- `error`

The ledger records payloads such as:

- request input
- update input
- final response output
- suspend reason
- cancel reason

It does not record every internal model turn.

### Daemon Logs

Verbose daemon logs give you the best live operational picture.

They show:

- runtime start and stop
- request enqueue
- dispatch start
- session launch and completion
- request update
- request response
- request suspension

Payload previews for request and response text are only included in verbose logging mode.

## Prompts That Work Well For Human Testing

Use prompts that make the desired test behavior explicit.

Good examples:

- "Do not change files. Inspect and summarize only."
- "If you need more context, suspend with a clear reason."
- "Delegate one read-only check to the right domain agent and report the result."
- "Work only in your owned paths and tell me what those paths appear to be."

Avoid using a first test pass to validate broad repo edits. The current backlog still calls out incomplete git controls, response validation, and richer lifecycle inspection.

## Current Limitations That A Tester Should Know

- Workforce is pre-alpha and headless.
- There is no dedicated app UI for workforce.
- Shared-working-tree git enforcement is not complete yet.
- Response validation against owned-path dirty state and attributable commits is not complete yet.
- `update` does not currently steer an active live session in place; it requeues.
- Runtime lifecycle inspection is still shallow compared with the intended state model.
- Config validation is still lighter than the long-term design.

Because of those gaps, the safest manual test plan is read-only inspection, explicit delegation, suspend/resume, cancellation, truncation, and replay after restart.

## Suggested Success Criteria

A reasonable human test pass should prove all of the following:

- workforce can be initialized for the repository
- the daemon can start exactly one runtime for that repository
- the root agent can receive work
- at least one domain agent can receive work
- the root agent can delegate
- a request can suspend and later resume through `update`
- the operator can cancel and truncate queued work
- the ledger provides a durable, readable audit trail
- restart replays the queue from ledger state

## Suggested Failure Notes To Capture

When you find a problem, record:

- the exact command you ran
- the request id
- the target agent id
- whether the daemon was started with verbose logging
- the relevant ledger lines
- whether the issue was routing, ownership, observability, lifecycle, or git-safety related

That is enough context to turn a manual test result into a bug report quickly.

## One Practical Warning

Do not use `bun run smoke:workforce` as your only test signal right now. The current manual testing path is more trustworthy for operator validation than the repo-wide smoke script.
