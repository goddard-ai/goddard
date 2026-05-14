# Component: SessionChatTranscript Remaining Work

- **Current Baseline:** `app/src/session-chat/` already has an app-owned ACP transcript path: `SessionChat` normalizes history and live messages, `buildSessionChatTranscript` produces discriminated transcript rows, and `Transcript` renders text rows, grouped tool rows, permission requests, plan updates, turn stops, older-history paging, and scroll restoration through the shared message-list virtualizer.
- **Planning Purpose:** Track the transcript gaps that still matter after the first ACP-native implementation, not the original from-scratch component build.
- **Non-Goal:** Do not revive the Pretext-backed transcript or document renderer at this stage. Text rendering should continue through the current markdown pipeline unless a separate implementation decision replaces it.

## Source-Backed Behavior To Preserve

- Keep transcript rows ACP-native instead of collapsing everything into generic role-tagged chat bubbles.
- Preserve stable identity for agent text rows, tool calls, permission requests, plan snapshots, and turn stop rows.
- Keep the transcript tied to the app-owned tab scroller rather than introducing nested transcript scrolling.
- Keep permission requests actionable inline and leave resolved permission cards in history.
- Keep plan updates as complete replacement snapshots, not incremental merges.
- Keep tool output compact in the transcript and route heavy review into dedicated surfaces.

## Near-Term Gaps

- **User message chunks:** Support ACP user-message chunk updates if the daemon begins emitting them separately from the initial prompt request.
- **Agent thoughts:** Preserve `agent_thought_chunk` events as their own transcript item type, even if the first presentation is collapsed or hidden by default.
- **Resource link actions:** Add click or keyboard affordances for local `resource_link` blocks so absolute paths and 1-based lines can open through the app's file or project routing once that routing exists.
- **Terminal attachments:** Replace terminal tool-content placeholders with a transcript-friendly attachment when terminal replay or live terminal attachment data is available.
- **Unsupported update diagnostics:** Keep unknown or malformed session updates from silently flattening into text; surface debug-safe diagnostics without polluting normal transcripts.

## Review Follow-Ons

- Add `SessionTurnChangeSummary` output after completed turns once local git snapshotting and provenance rules are implemented.
- Let turn summaries open the future `CodeDiffView` for full patches instead of inlining large diffs.
- Add row actions such as copy, jump to file, or open diff only after the primary row model stays stable.

## Capability-Gated Follow-Ons

- Render embedded resources, images, and audio only when the ACP session capabilities and app UX explicitly support those prompt content types.
- Add replay boundaries only when `session/load` or a similar history-resume protocol needs to distinguish replayed history from live continuation.
- Route `session_info_update`, mode changes, config option updates, and available commands into header, composer, or command surfaces rather than transcript body rows.
