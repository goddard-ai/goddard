# Session Turn History Design

## Overview

The daemon currently persists one `sessionMessages` record per session, with one ever-growing `messages: acp.AnyMessage[]` array. That shape is easy to bootstrap, but it is a poor fit for streamed prompt turns:

- every new message rewrites the full session history blob
- storage cost grows with total session age rather than the active turn
- `sessionHistory` returns the entire transcript at once
- the app has to reconstruct turn boundaries client-side from a flat transport log

This design replaces per-session message blobs with per-turn persistence. Completed turns are stored as immutable records. The active turn is held in memory and mirrored into one mutable draft record. History reads return paged turns instead of one flattened ACP array.

## Context

- The daemon already serializes prompt dispatch so one session has at most one active prompt turn at a time.
- The live `sessionMessage` subscription remains the real-time streaming surface. This design does not change the live stream contract.
- The app transcript and planned turn summary UI already think in prompt-turn units rather than one flat message log.
- The daemon currently derives slash-command suggestions from `available_commands_update` messages found in session history. If history stops being a raw audit log, that state must move elsewhere.
- ACP `loadSession` is optional. The installed ACP SDK documents that agents with `loadSession` should restore session context and stream conversation history back via notifications.
- Active-turn durability should not depend on ACP adapter capabilities. History semantics should remain the same across agents with and without `loadSession`.

## Goals

- Persist completed prompt turns as bounded durable records.
- Persist the active prompt turn as a mutable draft so daemon failure does not discard the whole in-progress turn.
- Keep the active prompt turn in memory and include it in latest-page history reads while the session is live or after an interrupted daemon restart.
- Page session history by turn rather than returning the entire session transcript.
- Coalesce high-frequency streamed agent text chunks in memory before persistence.
- Bound crash loss for the active turn to at most the most recent unflushed draft window, not the entire turn.
- Preserve enough ACP fidelity inside each turn record for transcript rendering, debugging, and turn-summary derivation.
- Keep live streaming behavior unchanged for `sessionMessage` subscribers.

## Non-Goals

- Preserve the current `sessionHistory` response shape.
- Persist every streamed chunk as its own durable record.
- Maintain a full raw ACP audit log for messages that never became part of a prompt turn.
- Guarantee zero-loss durability for every individual streamed chunk.
- Use different active-turn persistence rules depending on whether an agent supports `loadSession`.
- Solve durable storage for every session-scoped ACP update in this slice.

## Assumptions And Constraints

- The repository is pre-alpha. Breaking daemon IPC, schema, and SDK changes are acceptable when they simplify the forward-looking design.
- One live session may have queued prompts, but only one prompt is ever actively executing at a time.
- Prompt request ids are required for raw queued ACP prompts today, so a persisted turn can safely key itself to one prompt request id.
- A turn boundary is defined by the lifecycle of one dispatched `session/prompt` request, not by visible assistant text.
- `session/cancel` and `sessionSteer` may produce late tool updates before the cancelled prompt response arrives. Those late updates still belong to the cancelled turn.

## Terminology

- `Turn`
  - The daemon-owned history unit for one dispatched `session/prompt` request and its associated ACP traffic.
- `Completed Turn`
  - A turn whose prompt lifecycle ended with a JSON-RPC result or error from the agent.
- `Active Turn`
  - The single in-memory turn currently receiving streamed updates for one live session.
- `Turn Draft`
  - The mutable durable snapshot of the active turn stored in the Goddard database.
- `Turn Buffer`
  - The mutable in-memory structure the daemon appends to while a turn is active.
- `Draft Flush`
  - The daemon action that writes the current in-memory active turn into the durable draft record.
- `Turn Page`
  - One page of persisted turns returned by the history API, optionally followed by the newest in-progress turn.
- `Latest Page`
  - The history request with no cursor, which returns the newest persisted turns and appends the active turn or recovered draft when present.

## Proposed Design

### 1. Persistence Model

Replace the current `sessionMessages` kind with two kinds:

- `sessionTurns`
  - immutable completed turns
- `sessionTurnDrafts`
  - one mutable active-turn draft per session

Each persisted record stores one completed turn:

```ts
type DaemonSessionTurn = {
  sessionId: DaemonSessionId
  turnId: string
  sequence: number
  promptRequestId: string | number
  startedAt: string
  completedAt: string
  completionKind: "result" | "error"
  stopReason: DaemonSessionStopReason | null
  messages: acp.AnyMessage[]
}
```

Required indexes:

- `sessionId`
- `sessionId_sequence`

`sequence` is monotonically increasing within a session and is the stable ordering key for paging and transcript assembly.

Draft record:

```ts
type DaemonSessionTurnDraft = {
  sessionId: DaemonSessionId
  turnId: string
  sequence: number
  promptRequestId: string | number
  startedAt: string
  updatedAt: string
  messages: acp.AnyMessage[]
}
```

Required indexes:

- `sessionId`

The daemon enforces that at most one draft exists per session.

### 2. In-Memory Active Turn Buffer

`ActiveSession` gains an `activeTurn` buffer:

```ts
type ActiveTurnBuffer = {
  turnId: string
  sequence: number
  promptRequestId: string | number
  startedAt: string
  messages: acp.AnyMessage[]
}
```

The buffer is created when the daemon actually dispatches a prompt to the agent, not when the prompt is merely queued.

The buffer is mirrored into `sessionTurnDrafts` during the turn lifetime.

The buffer is finalized into `sessionTurns` only when the matching prompt request resolves with a JSON-RPC result or error.

### 3. Session-Scoped State That No Longer Lives In History

The current slash-command composer path depends on `available_commands_update` messages being present in raw history. Under turn paging, that dependency moves to session state.

This slice adds one durable `availableCommands` field to the session record. The daemon updates it whenever it receives an `available_commands_update`.

This design does not yet persist other session-scoped updates such as `session_info_update`, `current_mode_update`, or `config_option_update`. Those should remain out of turn history and can move to dedicated session-state persistence in a follow-on slice.

## API And Interface Specification

### IPC Schema

Replace the current history request payload:

```ts
type DaemonSessionIdParams = { id: DaemonSessionId }
```

with:

```ts
type GetSessionHistoryRequest = {
  id: DaemonSessionId
  cursor?: string
  limit?: number
}
```

Response:

```ts
type SessionHistoryTurn = {
  turnId: string
  sequence: number
  promptRequestId: string | number
  startedAt: string
  completedAt: string | null
  completionKind: "result" | "error" | null
  stopReason: DaemonSessionStopReason | null
  messages: acp.AnyMessage[]
}

type GetSessionHistoryResponse = SessionIdentity & {
  connection: SessionConnection
  turns: SessionHistoryTurn[]
  nextCursor: string | null
  hasMore: boolean
}
```

`cursor` is opaque to clients. The daemon may currently encode the oldest returned persisted `sequence`, but callers must not depend on that representation.

`turns` may end with one incomplete draft turn. In that case:

- `completedAt` is `null`
- `completionKind` is `null`
- `stopReason` is `null`

### SDK Surface

Change the SDK wrapper to match the paged contract:

- `session.history()` returns `GetSessionHistoryResponse`.
- `AgentSession.getHistory()` is replaced with `AgentSession.getHistoryPage()`.

If a caller still needs a flat ACP list for one loaded page, it can flatten `page.turns.flatMap((turn) => turn.messages)`.

### App Surface

The app transcript stops assuming that one history fetch returns the whole session. It loads the latest page first, renders the returned turns in ascending `sequence` order, and can request older pages when the user scrolls upward.

## Behavioral Semantics

### Turn Start

A new turn begins when `processPromptQueue()` dispatches a `session/prompt` message to the agent.

The daemon must:

1. allocate `sequence = previousCompletedSequence + 1`
2. allocate a stable `turnId`
3. create the `activeTurn` buffer
4. append the outbound prompt message to that buffer
5. create or overwrite the `sessionTurnDrafts` record for the session
6. write the prompt to the agent transport

Queued prompts that are cancelled before dispatch never create a turn record.

### Message Routing

While an active turn exists:

- outbound daemon-authored messages that affect the active prompt lifecycle are appended to the active turn
- inbound agent messages that occur before the prompt resolves are appended to the active turn
- late tool updates that arrive after daemon-owned cancellation but before the cancelled prompt response still belong to that cancelled turn

Messages that never belonged to a dispatched prompt turn are not persisted in `sessionTurns`.

Examples:

- queued prompt aborted before dispatch: not persisted as turn history
- slash-command availability update outside a prompt turn: update `session.availableCommands`, not `sessionTurns`

### Chunk Coalescing

The daemon coalesces consecutive `agent_message_chunk` updates with `content.type === "text"` inside the active turn buffer.

Coalescing is applied before draft writes and before completed-turn persistence:

- the live `sessionMessage` subscription still emits each raw chunk
- the durable draft stores the coalesced form
- persisted completed turns store the coalesced form

The first cut does not coalesce non-agent chunk types.

### Draft Flush

The daemon writes the active-turn draft eagerly for every agent.

Draft flush rules:

- flush immediately when the turn is created
- flush immediately on structural boundaries:
  - `tool_call`
  - `tool_call_update`
  - daemon-authored `session/cancel`
  - terminal prompt result or error
- debounce flushes for repeated streamed chunk traffic

The initial debounce target should be short and fixed, such as 100 ms.

This design intentionally trades zero-loss chunk durability for lower write amplification. A hard crash may lose the most recent unflushed chunk window, but not the entire active turn.

### Turn Completion

When the matching prompt request resolves:

- append the terminal JSON-RPC result or error to the active turn buffer
- flush the draft one final time
- derive `completionKind`
- derive `stopReason` when the terminal message is a prompt result
- write one `sessionTurns` record
- delete the matching `sessionTurnDrafts` record
- clear the in-memory `activeTurn`

If the prompt completes with a JSON-RPC error, the turn is still persisted as completed history with `completionKind: "error"` and `stopReason: null`.

### History Paging

The daemon pages persisted completed turns only. The active turn or recovered draft is appended only on the latest page.

Rules:

- no cursor:
  - return the newest `limit` persisted turns, ordered by ascending `sequence`
  - if the session is live and has an active turn, append the in-memory active turn after the persisted turns
  - otherwise, if a durable draft exists for the session, append the draft after the persisted turns
  - if daemon reconciliation has already promoted an interrupted draft into `sessionTurns`, return that incomplete turn as part of the persisted page and do not append a separate draft
  - if both a live active turn and a durable draft exist for the same turn, the in-memory active turn wins and the draft is not appended separately
- with cursor:
  - return persisted turns older than the cursor, ordered by ascending `sequence`
  - do not append the active turn or draft
- `limit` applies only to persisted turns
- `hasMore` and `nextCursor` are computed only from persisted turns

This makes the newest in-progress turn visible without changing older-page semantics or causing it to consume a persisted page slot.

### History Reads For History-Only Sessions

Archived or failed sessions with no live daemon runtime return persisted turns. During daemon reconciliation, any surviving draft is promoted into an incomplete turn row and the draft is deleted.

### Daemon Failure

If the daemon exits while a turn is active:

- completed turns remain durable
- the latest flushed draft remains durable
- on restart, session reconciliation may leave the session in a history-only or error state, promote the surviving draft into an incomplete turn row, and preserve it in history

For agents that support ACP `loadSession`, a future resume flow may reconcile replayed notifications with the existing draft. That replay is an improvement path, not a correctness requirement for turn durability.

## Architecture And Data Flow

### Write Path

1. Prompt is dequeued for dispatch.
2. Daemon creates `activeTurn`.
3. Daemon appends outbound prompt message to `activeTurn.messages`.
4. Daemon writes the initial draft record.
5. Agent streams updates.
6. Daemon appends each turn-scoped message to `activeTurn.messages`, coalescing agent text chunks in memory.
7. Daemon flushes the draft on debounce and structural boundaries.
8. Prompt resolves.
9. Daemon writes one `sessionTurns` record, deletes the draft, and clears `activeTurn`.

### Read Path

1. Client requests `sessionHistory({ id, cursor?, limit? })`.
2. Daemon loads the requested persisted `sessionTurns` page.
3. If this is the latest page and a live `activeTurn` exists, daemon appends that in-memory turn to the response.
4. Otherwise, if this is the latest page and a durable draft exists, daemon appends the draft to the response.
5. During daemon restart reconciliation, any surviving draft may first be promoted into `sessionTurns`, in which case the latest page reads it from persisted turns instead of appending a draft.
6. Daemon returns `turns`, `nextCursor`, and `hasMore`.

## Alternatives And Tradeoffs

### Keep One `sessionMessages` Blob Per Session

Rejected because it retains the current write amplification and all-or-nothing reads.

### Persist One Record Per ACP Message

Rejected because it optimizes write granularity at the cost of expensive turn reconstruction, more indexes, and worse transcript reads.

### Persist An Append-Only Raw ACP Log Plus A Derived Turn Table

Rejected for the first cut because it preserves more audit fidelity than the current app needs and doubles the amount of persistence logic. The daemon already has a live streaming surface for raw ACP observers.

### Persist Drafts Only For Agents Without `loadSession`

Rejected because it makes durability semantics depend on ACP adapter capability rather than the daemon's own storage contract. That would complicate testing, reconciliation, and user expectations for little practical benefit, since there is still at most one active turn per session.

### Selected Trade

The selected design trades zero-loss chunk durability for lower write amplification, and trades raw transport completeness for simpler turn-oriented storage. That is the right trade in this repository because the primary consumers are transcript UIs, per-turn summaries, and session inspection rather than exact ACP transport replay from local persistence.

## Failure Modes And Edge Cases

- Session with no completed turns and no active turn
  - `turns` is empty.
- Session with no completed turns and one active turn
  - latest page returns only the in-memory active turn.
- Session with no completed turns and one durable draft after daemon interruption
  - latest page returns only the recovered draft.
- Prompt cancelled after tool activity but before terminal result
  - tool updates remain attached to the cancelled turn until the prompt result arrives.
- Queued prompt aborted before dispatch
  - no turn record is created; the live raw subscriber still receives the terminal JSON-RPC error.
- Prompt result is an error rather than a normal `PromptResponse`
  - persist the turn with `completionKind: "error"`.
- Crash between completed-turn write and draft delete
  - history reads and reconciliation must prefer the completed turn and treat the same-turn draft as stale.
- Crash before the most recent debounced draft flush
  - the draft may miss the latest chunk window, but earlier turn content remains available.
- Very large tool payload inside one turn
  - memory pressure is still proportional to one active turn, not the entire session; this is an improvement, not a complete solution.

## Testing And Observability

Required invariants:

- persisted turn sequences are contiguous per session
- a prompt request id appears in at most one persisted turn plus at most one in-progress representation
- the latest history page appends the newest in-progress turn at most once
- at most one draft exists per session
- a completed turn wins over a stale same-turn draft
- active-turn coalescing never changes the live `sessionMessage` stream

Required tests:

- prompt completes and persists exactly one turn
- active turn creates and updates one draft record
- multiple turns page correctly across cursor boundaries
- latest page appends the in-progress turn
- latest page appends a recovered draft after daemon interruption
- older pages never append the in-progress turn
- queued prompt aborted before dispatch does not create a turn
- cancelled turn retains late tool updates before final prompt response
- stale same-turn drafts are ignored after completed-turn persistence
- `available_commands_update` continues to power slash suggestions after turn paging lands

Observability:

- add diagnostics for turn creation, draft flush, turn persistence, draft cleanup, and history page reads
- add diagnostics when queued prompts are intentionally excluded from turn persistence

## Rollout And Migration

### Code Rollout

Land the change in one daemon/API slice:

1. add `sessionTurns`, `sessionTurnDrafts`, and active-turn buffering
2. switch `sessionHistory` IPC and SDK types to paged turns
3. move slash-command suggestions off raw history scanning and onto `session.availableCommands`
4. update app history loading to page by turn

### Legacy Data

Because the repository is pre-alpha, the daemon does not migrate legacy `sessionMessages` blobs into the new turn model. Existing `sessionMessages` data is dropped.

## Open Questions

- Should `AgentSession` keep a convenience helper that flattens one page of turns back into `acp.AnyMessage[]`, or is explicit caller flattening clearer?
- Should the first cut also coalesce `agent_thought_chunk` and `user_message_chunk`, or should it stay limited to `agent_message_chunk` until a concrete need appears?
- Should the daemon expose draft-specific diagnostics or metadata in `sessionHistory`, or is the `completedAt: null` shape sufficient for clients?

## Ambiguities And Blockers

- AB-1 - Non-blocking - Draft flush debounce interval
  - Affected area: Behavioral Semantics / Performance
  - Issue: The document sets the first-cut debounce approach, but the exact interval is still a tunable choice.
  - Why it matters: It controls the tradeoff between write amplification and crash-window loss for chunk-heavy sessions.
  - Next step: Start with a small fixed interval such as 100 ms, then tune from observed daemon behavior.

- AB-2 - Non-blocking - Durable storage for non-command session-scoped updates
  - Affected area: Session state persistence
  - Issue: This design only moves `available_commands_update` out of history because it has a current daemon consumer. Other session-scoped updates still need a durable home once the app starts relying on them.
  - Why it matters: Future header or toolbar UI should not be forced back onto turn history for state reconstruction.
  - Next step: Follow with a session-state snapshot design if `session_info_update`, `current_mode_update`, or `config_option_update` become durable product requirements.
