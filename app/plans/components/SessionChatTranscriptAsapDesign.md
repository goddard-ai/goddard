# Component: SessionChatTranscript ASAP Design

- **Goal:** Define the technical design for the `ASAP` transcript components so the first production-grade `SessionChatTranscript` can represent a normal ACP coding turn correctly without depending on future review or replay features.
- **Scope:** Covers `SessionChatMessageList`, `TranscriptRowShell`, `PretextMarkdown`, `UserMessageRow`, `AgentMessageRow`, `ResourceLinkAttachment`, `ToolCallCard`, `ToolCallContentRenderer`, `ToolCallLocationList`, `PermissionRequestCard`, and `TurnStopState`.
- **Out of scope:** `PlanUpdatePanel`, `AgentThoughtRow`, turn change summaries, replay boundaries, embedded media, and richer row actions. Those stay in `Soon` or `Later`.

## Why This Slice Exists

- The transcript should stop behaving like a generic chat bubble list and become an ACP-native activity timeline.
- The first production milestone does not need every ACP feature, but it does need the core turn surfaces that make a coding session understandable:
  - what the user asked
  - what the agent replied
  - what tools ran
  - which files or terminals were involved
  - whether permission was requested
  - how the turn ended
- This design keeps those concerns explicit so later additions remain additive instead of stretching one message-bubble component past its limits.

## Core Design Principles

- **ACP-native records, not app-only messages**
  - `SessionChatState` should normalize ACP updates into transcript item records that preserve tool identity, permission lifecycle, and turn completion semantics.
- **One row type per visible concept**
  - User text, agent text, tool activity, permission requests, and stop states should not be disguised as one generic “message” type with flags.
- **One scroller per tab**
  - `SessionChatMessageList` should keep using the shared tab viewport and own virtualization only, not inner scrolling.
- **Stable identity across updates**
  - Tool calls, pending permissions, and message rows must survive incremental ACP updates without remounting into new transcript rows.
- **Compact transcript, deeper views elsewhere**
  - The transcript should explain the turn clearly, but heavy review surfaces such as full diffs belong in dedicated views later.

## Shared Transcript Model

- **Recommended normalized item shape**
  - `userMessage`
  - `agentMessage`
  - `toolCall`
  - `permissionRequest`
  - `turnStop`
- **Recommended shared fields**
  - `itemId`
  - `turnId`
  - `createdAt`
  - `status`
  - optional `isPending`
- **Identity rules**
  - One ACP chunk stream should update an existing row whenever ACP identity says it is the same visible artifact.
  - `toolCallId` should anchor one `ToolCallCard`.
  - Permission requests should anchor one `PermissionRequestCard`.
  - Stop state should attach to one prompt turn, not to a specific message row.
- **Ordering rules**
  - Rows should remain in transcript order according to the turn timeline, not grouped by component type.
  - A user turn can contain a user row, then one or more tool rows, then one or more agent rows, then a turn stop row.
  - `resource_link` blocks belong inside the user row unless a future design introduces a richer prompt-composition artifact group.

## Component Roles

## `SessionChatMessageList`

- **Responsibility**
  - Own virtualization, visible-range management, external scroller attachment, and scroll restoration for transcript rows.
- **What it should know**
  - row identity
  - row order
  - approximate and measured row heights
  - whether the transcript is currently bottom-pinned or history-anchored
- **What it should not know**
  - ACP semantics
  - markdown parsing
  - tool-card rendering
  - permission logic
- **Behavioral contract**
  - Accept already-normalized transcript items.
  - Render rows through a typed row renderer, not through ad hoc JSX branching in the list itself.
  - Preserve per-tab scroll position and transcript anchoring when tabs are switched.
  - Treat bottom anchoring as a chat-specific policy layered on top of a normal top-down virtualized list.
- **Why it matters first**
  - Every later row type inherits its scroll, mount, and viewport behavior from this surface. If this layer is unstable, every transcript component feels unstable.

## `TranscriptRowShell`

- **Responsibility**
  - Provide the shared visual and structural wrapper around all major transcript rows.
- **Shared concerns it should own**
  - horizontal rhythm
  - row spacing
  - alignment rules
  - metadata placement
  - keyboard focus target
  - state badges such as pending, running, completed, or cancelled
- **Recommended slots**
  - `meta`
  - `body`
  - optional `footer`
  - optional `sidecar`
- **Alignment model**
  - User-authored rows can remain visually distinct and right-leaning if desired.
  - Agent and tool rows should feel left-aligned and stable because they make up most of the coding activity history.
  - System-like rows such as `TurnStopState` can use a more neutral centered or low-emphasis presentation without inventing a separate layout stack.
- **Why it matters first**
  - Without a shared shell, the transcript will drift into separate bubble designs that look unrelated even when they are part of the same turn.

## `PretextMarkdown`

- **Responsibility**
  - Be the default rich-text renderer for paragraph-heavy transcript content.
- **Primary consumers**
  - `UserMessageRow`
  - `AgentMessageRow`
  - `ToolCallContentRenderer` for text-like tool output
- **Content model**
  - Paragraph-like markdown blocks should route through Pretext-backed layout.
  - Non-paragraph blocks such as fenced code, rules, and list markers can remain normal DOM blocks wrapped by the same component.
  - `resource_link` blocks should stay outside markdown and render through `ResourceLinkAttachment`.
- **Rendering rules**
  - Markdown should be treated as the default transcript text surface, especially for agent output.
  - The renderer should stay deterministic and transcript-safe rather than aspiring to full document-editor fidelity.
  - Large blocks should still participate in the same row-height contract so virtualization remains accurate.
- **Boundary**
  - `PretextMarkdown` should solve text presentation, not row identity, tool semantics, or permission semantics.

## `UserMessageRow`

- **Responsibility**
  - Represent the human prompt for one turn as ACP actually describes it.
- **Supported ASAP content**
  - `text`
  - `resource_link`
- **Structure**
  - One `TranscriptRowShell`
  - one ordered content stack inside the shell
  - markdown-backed text blocks for prompt prose
  - `ResourceLinkAttachment` blocks for linked context
- **Important rules**
  - Preserve ACP block order.
  - Do not flatten `resource_link` blocks into plain markdown text.
  - Keep the row understandable as “the prompt that started this turn,” not as an arbitrary transcript paragraph.
- **Why it matters**
  - If the prompt is normalized into a plain string too early, the transcript loses the file and context references that often explain the rest of the turn.

## `AgentMessageRow`

- **Responsibility**
  - Represent the user-visible agent response stream.
- **Structure**
  - One `TranscriptRowShell`
  - markdown-rendered body through `PretextMarkdown`
  - optional pending state while the row is still incomplete
- **Rendering rules**
  - The agent row should be optimized for readability over novelty because it is the main explanatory surface in the transcript.
  - Partial or incrementally arriving content should still map onto one stable row identity.
  - Tool activity should not be squeezed into this row. Tool output belongs in adjacent tool rows.
- **Why separate from tool rows**
  - Agent narrative and tool execution are different concepts in ACP. Combining them would make the transcript harder to scan and harder to extend later.

## `ResourceLinkAttachment`

- **Responsibility**
  - Render ACP `resource_link` prompt blocks as structured transcript attachments.
- **Required information**
  - title or label when available
  - absolute path or resource identifier
  - 1-based line reference when present
  - a clear distinction between local file context and non-file resources
- **Presentation goals**
  - Feel like linked prompt context, not like another chat message.
  - Stay compact enough that multiple attachments can appear inside one user row without dominating the whole turn.
  - Preserve enough path detail for a developer to trust what context was given to the agent.
- **Boundary**
  - This component is about prompt context display. It should not become a full file preview surface.

## `ToolCallCard`

- **Responsibility**
  - Give one ACP tool call a stable transcript row with durable identity across updates.
- **Required header information**
  - tool title
  - tool kind
  - status
  - optional short summary when ACP provides one
- **Body composition**
  - `ToolCallContentRenderer`
  - optional `ToolCallLocationList`
- **Lifecycle expectations**
  - A tool card should begin pending or running.
  - It should update in place as `tool_call_update` events arrive.
  - It should remain visible after completion so the transcript preserves the turn narrative.
- **Why it matters first**
  - Tool activity is the core of a coding agent transcript. Without stable tool cards, the session reads like a chat log instead of a work log.

## `ToolCallContentRenderer`

- **Responsibility**
  - Render the content area for ACP tool output without coupling layout to specific tool names.
- **Supported ASAP variants**
  - `content`
  - `diff`
  - `terminal`
- **`content` variant**
  - Best for human-readable summaries, logs, or structured text.
  - Should default to text or markdown treatment rather than raw preformatted output unless the tool semantics clearly require otherwise.
- **`diff` variant**
  - Should provide a compact inline preview that confirms which file or patch area changed.
  - Should stop short of full multi-file review UI. The transcript should summarize, not replace a dedicated diff surface.
- **`terminal` variant**
  - Should reserve space for a terminal attachment keyed by `terminalId`.
  - The transcript row should treat the terminal as part of tool history, not as a separate page-level panel.
- **Why this variant split matters**
  - ACP already defines the content categories. The renderer should follow that contract instead of branching on tool names or app-specific heuristics.

## `ToolCallLocationList`

- **Responsibility**
  - Show the file locations associated with one tool call.
- **Required information**
  - absolute path
  - 1-based line number
  - multiple locations in the same tool call when present
- **Presentation goals**
  - Stay secondary to the tool card body.
  - Be easy to skim for “what files did this touch or inspect?”
  - Preserve exactness; path and line fidelity matter more than decorative formatting.
- **Recommended behavior**
  - Show locations in a compact list below the main tool content.
  - Allow later drill-in behavior without requiring it for the first transcript milestone.

## `PermissionRequestCard`

- **Responsibility**
  - Represent ACP permission requests inline, as part of the turn history.
- **Why inline matters**
  - Permission is part of the active coding workflow. Routing it entirely out of band would make the transcript incomplete.
- **States it should support**
  - pending
  - approved
  - rejected
  - cancelled
- **Visible information**
  - what action needs approval
  - why the request happened
  - which choices are available
  - the final outcome once resolved
- **Behavioral expectation**
  - Once resolved, the card should remain as immutable history for that turn.
  - If the turn is cancelled before the user answers, the card should visibly transition to a cancelled state rather than disappearing.

## `TurnStopState`

- **Responsibility**
  - Make the end of a prompt turn explicit.
- **Supported ASAP reasons**
  - `end_turn`
  - `max_tokens`
  - `max_turn_requests`
  - `refusal`
  - `cancelled`
- **Presentation role**
  - This should act as a small terminal artifact for the turn, not another full message bubble.
  - It should help the user distinguish “the agent is still working” from “the turn has concluded.”
- **Why it should be separate**
  - ACP stop reasons belong to the turn lifecycle, not to the final agent paragraph or final tool card.
  - `cancelled` especially needs its own clear record because late tool updates can still arrive before the prompt resolves.

## How The ASAP Components Work Together

- One prompt turn should read as a compact activity slice:
  - `UserMessageRow`
  - zero or more `ToolCallCard`
  - zero or more `AgentMessageRow`
  - zero or more `PermissionRequestCard`
  - `TurnStopState`
- This order should reflect the actual turn timeline, not a rigid role grouping.
- `TranscriptRowShell` should make these row types feel related even though their bodies differ.
- `SessionChatMessageList` should virtualize the row sequence without owning any ACP-specific logic.
- `PretextMarkdown` should provide the common text-rendering language across user, agent, and tool text surfaces.

## State Ownership Boundaries

- `SessionChatTranscript`
  - presentational composition only
  - maps normalized transcript items to row components
- `SessionChatMessageList`
  - virtualization and scroll behavior only
- `SessionChatState`
  - ACP normalization, item identity, turn grouping, and future streaming updates
- dedicated host or desktop adapters
  - terminal attachment plumbing
  - file-opening actions
  - future diff-view routing

## Non-Goals For The ASAP Design

- Do not introduce turn change summaries yet.
- Do not collapse tool output into agent prose for convenience.
- Do not treat ACP permission flow as modal-only UI with no transcript record.
- Do not require full replay or history-loading support before the first correct live-turn transcript lands.
