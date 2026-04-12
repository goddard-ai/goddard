# Component: SessionChatTranscript Tool Call Surfacing

- **Goal:** Define the dedicated design for surfacing ACP tool calls in `SessionChatTranscript` so the session view reads like coding work history instead of flattened assistant text.
- **Context:** The app already stores raw ACP history and the daemon already persists `tool_call` and `tool_call_update` events, but the current transcript path in `app/src/session-chat/chat.ts` collapses all `session/update` payloads into generic assistant text.
- **Inputs:** `SessionChatTranscriptAcpRequirements`, `SessionChatTranscriptAsapDesign`, `SessionChatTranscriptPriorities`, `SessionChatState`, and `SessionTurnChangeSummary`.
- **Out of scope:** Plan updates, permission requests, agent-thought rendering, turn change summaries, and any new daemon or SDK protocol surface. This slice is about tool-call surfacing inside the transcript with existing ACP data.

## Overview

`SessionChatTranscript` needs first-class tool rows because ACP models tool activity as durable session updates, not as text embedded inside the assistant reply. The current transcript implementation loses that distinction by recursively stringifying `session/update` payloads. That produces two failures:

1. The transcript cannot show tool status, locations, diff previews, or terminal attachments with stable identity.
2. `tool_call_update` cannot update an existing visible artifact in place because there is no tool-call row model to target.

This design introduces one dedicated normalized transcript item for each ACP tool call and defines the state, merge, ordering, and rendering semantics required to keep that item correct during both history replay and live updates.

## Goals

- Surface each ACP `tool_call` as one stable transcript row keyed by `toolCallId`.
- Merge `tool_call_update` notifications into the existing row without remounting or reordering it.
- Render the ACP tool-call fields that matter for coding sessions:
  - title
  - kind
  - status
  - content
  - locations
- Support the ACP tool content variants that the broader transcript plan already expects:
  - `content`
  - `diff`
  - `terminal`
- Keep transcript behavior correct for both history replay and live session updates.
- Keep the row compact enough for transcript use while preserving exact path and line fidelity.

## Non-Goals

- Replacing `CodeDiffView` with large inline review UI.
- Inventing app-only tool event schemas or daemon extensions.
- Solving turn change summaries in the same slice.
- Adding product-terminal ownership to the app host.
- Rendering arbitrary raw tool payloads in a fully generic inspector UI.

## Success Criteria

- A completed coding turn with tool activity shows separate tool rows instead of only assistant `"Update"` bubbles.
- Replayed history and live updates produce the same normalized tool-call row sequence for the same ACP message stream.
- A `tool_call_update` that changes status or content updates an existing visible row rather than appending a new one.
- File locations stay absolute and 1-based in the transcript.
- Cancelled turns can still show final tool updates before the prompt resolves with `cancelled`.

## Assumptions and Constraints

- The daemon already persists raw ACP history and returns it through `goddardSdk.session.history`; no new SDK surface is required for the first tool-call transcript milestone.
- `SessionChatState` is the correct owner for ACP normalization. `SessionChatTranscript` should render normalized rows, not interpret raw ACP messages.
- ACP `tool_call_update` payloads may be partial. Scalar fields merge by replacement when present. `content` and `locations` collections replace prior collections when present.
- The app must preserve ACP naming, absolute file paths, and 1-based line semantics.
- Tool rows must remain additive to the broader transcript model and must not depend on `SessionTurnChangeSummary`.
- Terminal-related behavior must stay compatible with daemon-managed terminal ownership. The transcript may reference or attach to a terminal session, but it must not create a separate app-host PTY lifecycle for tool calls.

## Terminology

- **Tool row**
  - The normalized transcript item that represents one ACP tool call.
- **Tool card**
  - The rendered UI row for one tool row.
- **Tool content payload**
  - The ACP `content` collection attached to a tool call, which may contain `content`, `diff`, or `terminal` variants.
- **Turn**
  - The client-local prompt lifecycle anchored to one `session/prompt` request id and completed by that request's final response.
- **First sighting order**
  - The transcript order position assigned when a tool row is first created. Later updates do not change that position.

## Proposed Design

### High-Level Decision

Create a dedicated tool-call normalization path inside `SessionChatState` and feed the result to a dedicated `ToolCallCard` row in `SessionChatTranscript`.

The transcript pipeline should become:

1. Raw ACP messages enter `SessionChatState` from history replay or live subscription.
2. `SessionChatState` assigns each message to a client-local turn record.
3. `SessionChatState` converts `tool_call` and `tool_call_update` messages into normalized tool rows keyed by `toolCallId`.
4. `SessionChatTranscript` renders those normalized rows through `ToolCallCard`.
5. `ToolCallCard` delegates the body to `ToolCallContentRenderer` and `ToolCallLocationList`.

The current `chat.ts` string-flattening path should not be extended for tool-call support. Tool calls need structural normalization, not more text extraction heuristics.

### Normalized Tool Row Contract

Recommended normalized shape:

```ts
type TranscriptToolCallItem = {
  itemId: string
  kind: "toolCall"
  turnId: string
  toolCallId: string
  orderKey: number
  createdAt: number
  updatedAt: number
  title: string | null
  toolKind: "read" | "edit" | "delete" | "move" | "search" | "execute" | "think" | "fetch" | "switch_mode" | "other"
  status: "pending" | "in_progress" | "completed" | "failed"
  summary: string | null
  content: readonly ToolCallContentItem[]
  locations: readonly ToolCallLocationItem[]
  rawInput: unknown | null
  rawOutput: unknown | null
  sourceEvent: "tool_call" | "tool_call_update"
  isCancelledView: boolean
}
```

Supporting records:

```ts
type ToolCallContentItem =
  | { type: "content"; content: readonly unknown[] }
  | { type: "diff"; path: string | null; oldText: string | null; newText: string | null }
  | { type: "terminal"; terminalId: string }

type ToolCallLocationItem = {
  path: string
  line: number | null
}
```

Contract notes:

- `itemId` should be stable for the life of the row and can be derived as `${turnId}:tool:${toolCallId}`.
- `turnId` is client-local. For live turns, use the active unresolved prompt request id. For replay, derive the same turn grouping while walking history in order.
- `orderKey` reflects first sighting order within the transcript. It does not change on later updates.
- `summary` is optional and should only hold a concise line from ACP if one exists. Do not synthesize summaries by flattening arbitrary raw payloads.
- `isCancelledView` is presentation-only and lets the UI show a cancelled or interrupted state when the enclosing turn is cancelled before the tool row reaches a terminal ACP status.

## API and Interface Specification

### `SessionChatState`

This slice adds responsibilities to the existing state plan but does not require a new top-level state module.

Required additions:

- Keep a per-session map of active and historical turn records keyed by prompt request id.
- Keep a per-turn map of tool rows keyed by `toolCallId`.
- Expose transcript items as a discriminated union that includes `toolCall`.
- Apply the same normalization pipeline to:
  - fetched history
  - live `session/update` notifications

Recommended internal helpers:

- `startPromptTurn(requestId, prompt)`
- `appendToolCallUpdate(turnId, update)`
- `finalizePromptTurn(requestId, stopReason)`
- `markPendingToolRowsCancelled(turnId)`

State invariants:

- At most one tool row exists per `turnId + toolCallId`.
- A `tool_call_update` without an existing row creates a row in degraded mode instead of being dropped.
- Transcript rows preserve stream order even when the same tool row is updated many times.
- Final prompt cancellation may coexist with one or more late-arriving tool updates before the prompt response resolves.

### `SessionChatTranscript`

`SessionChatTranscript` should accept already-normalized transcript items and route `toolCall` items to `ToolCallCard`.

Behavioral contract:

- Do not attempt to derive tool rows from assistant text.
- Do not reorder tool rows when later updates arrive.
- Preserve the same row key across rerenders so status and content changes update in place.

### `ToolCallCard`

Responsibilities:

- Render tool metadata in the row shell.
- Render state badges from normalized status.
- Render zero or more content payloads in order.
- Render a compact locations list when locations exist.
- Surface cancelled-turn presentation without rewriting ACP status history.

Required visible fields:

- title
- kind
- status
- optional summary

### `ToolCallContentRenderer`

Responsibilities:

- Render the normalized `content` collection in order.
- Delegate each payload variant to a deterministic presentation path.
- Keep transcript-safe layout and row-height behavior.

Variant rules:

- `content`
  - Render as transcript-safe markdown or structured text content.
  - Use the same text renderer family as agent-visible transcript text when practical.
- `diff`
  - Render a compact inline patch preview with path context.
  - Prefer file identity and a short changed-region preview over full review chrome.
- `terminal`
  - Render a terminal attachment surface keyed by `terminalId` when available.
  - Until a live attachment surface exists, render a compact terminal reference state that clearly shows the `terminal` content type and allows later drill-in.

### `ToolCallLocationList`

Responsibilities:

- Render ACP file locations as a secondary list below the tool content.
- Preserve exact path and line values.
- Avoid deduplicating distinct ACP locations unless they are byte-for-byte identical.

## Behavioral Semantics

### Turn Association

Tool rows belong to the currently active prompt turn.

Replay semantics:

- When replaying history, a `session/prompt` request opens a turn keyed by its request id.
- Subsequent `session/update` notifications belong to the most recent unresolved prompt turn.
- The matching prompt response closes that turn.

Live semantics:

- When `SessionChatState` sends `session/prompt`, it creates the turn record immediately.
- Incoming `tool_call` and `tool_call_update` notifications attach to that active turn until the prompt request resolves.

This design relies on the daemon's serialized prompt execution model. If queued prompts appear in history before the active prompt resolves, only the currently executing unresolved prompt receives tool updates.

### Tool Row Creation

On `tool_call`:

- If `toolCallId` is new for the active turn, create a new tool row.
- Seed scalar fields from the ACP payload when present.
- Seed `content` and `locations` from the ACP payload when present, otherwise default to empty lists.
- Assign `orderKey` from the current transcript sequence counter.

On `tool_call_update`:

- If the row already exists, merge into it.
- If the row does not exist, create it in degraded mode with the current `toolCallId`, attach it to the active turn, and mark `sourceEvent` as `tool_call_update`.

The degraded creation path is intentional. It is safer to show a partially described tool row than to silently discard ACP tool history.

### Merge Rules

Scalar fields:

- Replace only when the new field is present.
- Preserve the prior value when the update omits the field.

Collection fields:

- `content` replaces the previous `content` collection when present.
- `locations` replaces the previous `locations` collection when present.

Ordering:

- The row keeps its original `orderKey`.
- Content payloads render in the order supplied by ACP.

Status rules:

- ACP status remains the source of truth when present.
- If the enclosing prompt resolves with `cancelled` and a tool row never reached `completed` or `failed`, the UI may render a cancelled badge through `isCancelledView`.
- Do not rewrite a `failed` tool row to `cancelled` just because the turn later cancels.

### Cancellation Semantics

ACP allows final tool updates to arrive after cancellation starts and before the original `session/prompt` request resolves. The transcript must therefore:

- keep existing tool rows visible during cancellation
- continue merging late `tool_call_update` messages
- mark unfinished rows as cancelled in the UI only after the prompt resolves with `cancelled`

This keeps the transcript faithful to ACP and avoids showing a completed prompt turn while tool rows are still changing.

### Diff Rendering Semantics

`diff` payloads are transcript evidence, not the full review surface.

Rules:

- Show the file path when available.
- Show a compact patch excerpt or changed-region summary.
- If multiple diff payloads exist in one tool row, render them in ACP order.
- Do not aggregate multiple diff payloads into one synthetic patch in the transcript row.

Full review remains the responsibility of `CodeDiffView` and later turn-summary surfaces.

### Terminal Rendering Semantics

Terminal payloads are part of tool history, not a separate transcript row type.

Rules:

- Keep the `terminal` payload inside the tool row that created it.
- Key any attachment state by `terminalId`.
- If a live terminal viewport is unavailable, render a compact terminal reference instead of dropping the payload.
- A released or exited terminal may remain visible as historical transcript content.

## Architecture and Data Flow

### Current Path

1. `SessionChatView` fetches session history.
2. `chat.ts` walks the raw ACP messages.
3. Any `session/update` payload is recursively flattened into strings.
4. The transcript renders plain role-tagged text bubbles.

This is insufficient because tool activity has identity and partial update semantics that text flattening cannot preserve.

### Proposed Path

1. `SessionChatState` or the history-normalization path reads ACP messages in order.
2. `session/prompt` opens or identifies the active turn.
3. `tool_call` and `tool_call_update` are normalized into per-turn tool rows keyed by `toolCallId`.
4. Normalized transcript items are emitted in transcript order.
5. `SessionChatTranscript` maps the `toolCall` kind to `ToolCallCard`.
6. `ToolCallCard` renders:
   - header metadata
   - `ToolCallContentRenderer`
   - optional `ToolCallLocationList`
7. Later transcript slices such as `SessionTurnChangeSummary` can reference the same tool rows without reparsing raw ACP messages.

## Alternatives and Tradeoffs

### Alternative: Keep flattening tool calls into assistant text

Rejected.

- Simpler short-term.
- Incorrect for ACP semantics.
- Prevents in-place updates and stable row identity.
- Makes locations, diff payloads, and terminal payloads effectively unusable.

### Alternative: Show all tool activity only in a side panel

Rejected.

- Preserves transcript cleanliness.
- Breaks the end-to-end turn narrative.
- Makes the active coding workflow harder to follow during live execution.

### Alternative: Collapse all tool activity into one per-turn summary row

Rejected for the tool-call slice.

- Compact UI.
- Too lossy during live execution.
- Conflicts with ACP's per-tool identity and partial update model.

### Selected Tradeoff

Use one row per tool call.

- More transcript rows.
- Much better correctness, traceability, and incremental update behavior.
- Keeps the transcript faithful to ACP while still leaving heavy review to dedicated views.

## Failure Modes and Edge Cases

- `tool_call_update` arrives before `tool_call`
  - Create a degraded row and merge future data into it.
- Tool row has no title
  - Render a fallback label based on `toolKind`, not raw JSON.
- Tool row has no content and no locations
  - Still render metadata and status; empty body is valid.
- Tool row contains multiple content payload variants
  - Render each payload in ACP order inside one card.
- Tool row path is relative or line is invalid
  - Preserve the raw value in debug logging, but do not normalize it into a fake absolute path or 0-based line number.
- Prompt cancels while a tool row is still `in_progress`
  - Keep the row, continue processing late updates, then mark cancelled presentation after the prompt resolves.
- History replay contains rows from older agent behavior with sparse fields
  - Normalize best-effort and avoid dropping the row.

## Testing and Observability

### Unit Tests

- history replay creates one tool row from one `tool_call`
- repeated `tool_call_update` messages update the same row
- `content` replacement semantics do not append stale payloads
- `locations` replacement semantics do not append stale locations
- cancelled turns preserve late tool updates before final cancelled presentation
- degraded `tool_call_update` creation path renders a visible row

### Component and Fixture Tests

- `ToolCallCard` renders each ACP status variant correctly
- `ToolCallContentRenderer` renders `content`, `diff`, and `terminal` payloads deterministically
- location rows preserve absolute paths and 1-based lines

### Debugging Support

- Add transcript debug fixtures that include:
  - one running tool
  - one tool updated to completion
  - one failed tool
  - one cancelled-turn tool
  - one diff preview
  - one terminal payload

## Implementation Phases

### Phase 1

- Replace the current `chat.ts` flattening path for tool-call events with normalized transcript items.
- Render `ToolCallCard`, `ToolCallContentRenderer`, and `ToolCallLocationList` from fetched history and fixture data.

### Phase 2

- Reuse the same normalization for live session updates in `SessionChatState`.
- Verify that rerender behavior keeps row identity stable during streaming updates.

### Phase 3

- Tighten terminal attachment behavior and connect deeper drill-in affordances where existing terminal or diff surfaces already support them.
- Let `SessionTurnChangeSummary` consume the same normalized tool-call records instead of reparsing raw ACP messages.

## Open Questions

- Should the first visible tool-row rollout happen directly inside a new `SessionChatState` pipeline, or is a temporary history-only normalization pass acceptable while live subscription wiring is still absent?
- How much inline diff excerpt is enough before the transcript starts duplicating `CodeDiffView` instead of complementing it?
- Should duplicate identical locations remain repeated when ACP sends them more than once, or should the renderer collapse only exact duplicates for scanability?

## Ambiguities and Blockers

- AB-1 - Non-blocking - Terminal attachment presentation depth
  - Affected area: `ToolCallContentRenderer` / terminal payloads
  - Issue: The broader transcript plans require `terminal` support, but the app's terminal planning is still centered on dedicated terminal views rather than an inline transcript attachment.
  - Why it matters: This affects how rich the `terminal` variant can be on day one, but it does not block shipping stable tool-call rows, statuses, diff previews, or locations.
  - Next step: Ship a compact terminal reference state keyed by `terminalId` in the first tool-call transcript rollout, then upgrade it to a live or replayable attachment when the shared terminal surface is ready.

- AB-2 - Non-blocking - Temporary coexistence with `chat.ts`
  - Affected area: normalization ownership
  - Issue: The current app still routes session history through `app/src/session-chat/chat.ts`, which is structurally incompatible with ACP-native tool rows.
  - Why it matters: Implementation may need a short-lived bridge while `SessionChatState` becomes the canonical transcript owner.
  - Next step: Treat any bridge as transitional only and keep the normalized tool-row contract owned by `SessionChatState`, not by `chat.ts`.

## Appendix: Minimal ACP Example

```json
{ "method": "session/prompt", "id": "prompt-1", "params": { "sessionId": "ses_1", "prompt": [{ "type": "text", "text": "review the patch" }] } }
{ "method": "session/update", "params": { "sessionId": "ses_1", "update": { "sessionUpdate": "tool_call", "toolCallId": "tool-1", "title": "Read diff", "kind": "read", "status": "in_progress", "locations": [{ "path": "/repo/src/app.ts", "line": 12 }] } } }
{ "method": "session/update", "params": { "sessionId": "ses_1", "update": { "sessionUpdate": "tool_call_update", "toolCallId": "tool-1", "status": "completed", "content": [{ "type": "content", "content": [{ "type": "text", "text": "Loaded file and diff context." }] }] } } }
```

Expected transcript behavior:

- one user turn opens for `prompt-1`
- one tool row appears for `tool-1`
- the second ACP update mutates the existing row instead of adding a new one
- the row keeps its original position and now shows `completed`
