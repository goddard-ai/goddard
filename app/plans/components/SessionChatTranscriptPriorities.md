# Component: SessionChatTranscript Priorities
- **Goal:** Prioritize the transcript component work needed to turn `SessionChatTranscript` from a debug-only message list into a production-ready ACP session surface.
- **Inputs:** `SessionChatTranscriptAcpRequirements`, `SessionTurnChangeSummary`, `PretextMarkdown`, and `CodeDiffView`.
- **Ordering principle:**
  - `ASAP` means the transcript cannot credibly represent a normal live ACP turn without the component.
  - `Soon` means the component adds important coding-workspace review value once the live turn is already trustworthy.
  - `Later` means the component is capability-gated, replay-oriented, or polish-heavy enough that it should not block the first serious transcript rollout.

## ASAP

- `SessionChatMessageList`
  - Keep the transcript-specific Virtuoso wrapper stable first so every later row type inherits the same external scroller, anchoring, and virtualization behavior.
- `TranscriptRowShell`
  - Add one shared row wrapper for spacing, alignment, metadata slots, and keyboard targeting so the transcript does not become a pile of one-off bubbles.
- `PretextMarkdown`
  - Make Markdown the default text renderer for transcript paragraphs so user and agent text, lists, and code blocks stop being treated as plain wrapped strings.
- `UserMessageRow`
  - Render ACP `user_message_chunk` updates and baseline prompt content blocks correctly.
- `AgentMessageRow`
  - Render ACP `agent_message_chunk` updates as the main visible answer surface.
- `ResourceLinkAttachment`
  - Support ACP `resource_link` prompt blocks with absolute paths and 1-based line references.
- `ToolCallCard`
  - Give each `tool_call` and `tool_call_update` a stable identity and status shell.
- `ToolCallContentRenderer`
  - Support the ACP tool content variants that will definitely show up in coding sessions:
    - `content`
    - `diff`
    - `terminal`
- `ToolCallLocationList`
  - Show file read and edit locations inline so the turn is inspectable without opening a separate panel for everything.
- `PermissionRequestCard`
  - Keep ACP approval requests inside the active turn instead of splitting the session into an out-of-band flow.
- `TurnStopState`
  - Render final stop reasons such as `end_turn`, `refusal`, `max_tokens`, and `cancelled` so the end of a turn is unambiguous.
- **Why this group:** Without these components, the transcript cannot faithfully represent a normal ACP coding session, even if the styling looks finished.

## Soon

- `PlanUpdatePanel`
  - Render ACP plan snapshots as first-class transcript items once the row model is stable.
- `AgentThoughtRow`
  - Preserve ACP `agent_thought_chunk` support even if the product later chooses to hide or collapse thoughts.
- `TurnBoundary`
  - Add a clear turn-completion boundary so transcript rows and end-of-turn artifacts are visually grouped.
- `TurnChangeSummaryCard`
  - Append one compact end-of-turn change summary once `SessionTurnChangeSummary` can finalize turn artifacts.
- `TurnChangedFileList`
  - Show the changed file set without forcing a full inline patch.
- `TurnDiffProvenanceBadge`
  - Make it explicit whether the summary came from local git, ACP tool diffs, or a weaker fallback.
- `TurnSummaryOpenDiffAction`
  - Route deeper review into `CodeDiffView` instead of bloating the transcript.
- `TurnSummaryWarningNote`
  - Call out dirty-worktree, partial-attribution, or paths-only cases without hiding them.
- **Why this group:** These components make completed turns reviewable and trustworthy as coding work, but they depend on the baseline turn model already being correct.

## Later

- `EmbeddedResourceAttachment`
  - Add only when ACP embedded context is actually enabled in the app.
- `ImageAttachment`
  - Add only when ACP image prompt content is in scope.
- `AudioAttachment`
  - Add only when ACP audio prompt content is in scope.
- `SessionLoadReplayBoundary`
  - Distinguish replayed history from live continuation after `session/load` lands.
- `TranscriptRowActions`
  - Add per-row affordances such as copy, reopen diff, or jump-to-file after the main row types are stable.
- `ExpandedToolDiffPreview`
  - Consider richer inline diff previews only after `CodeDiffView` is established as the main heavy-review surface.
- **Why this group:** These are real needs, but they are either capability-gated or additive enough that they should not block the first production transcript milestone.

## Implementation Sequence

- Build the `ASAP` group first against injected fake transcript data and ACP-shaped row records, not ad hoc string messages.
- Land the `Soon` group only after turn grouping and stop-state handling are stable, because turn change summaries depend on reliable turn boundaries.
- Keep `Later` components behind capability checks or clear follow-on plans so the transcript does not overfit to features the app is not ready to ship yet.

## Non-Goals For This Slice

- Do not inline full multi-file git patches directly into the transcript by default.
- Do not block the transcript rollout on embedded media capabilities that ACP only exposes when explicitly enabled.
- Do not reintroduce an `@assistant-ui/react` integration boundary; the transcript should stay app-owned and ACP-native.
