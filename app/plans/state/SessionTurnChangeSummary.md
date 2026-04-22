# State Module: SessionTurnChangeSummary

- **Goal:** Show a reliable per-turn summary of changed files and diff output at the end of each ACP prompt turn, even though ACP does not define a standardized end-of-turn change summary.
- **ACP gap:** ACP gives the client `tool_call`, `tool_call_update`, tool-call `diff` content, tool-call `locations`, and the final `stopReason`, but it does **not** define a single “here are the changed files for this turn” payload or a canonical git patch emitted at turn completion.
- **Why now:** `SessionChatTranscript` will need an end-of-turn artifact summary if the app wants to feel like a serious coding workspace instead of a chat log. Relying on ad hoc message parsing later will make the transcript model brittle.

## Proposed Ownership

- **State module:** `SessionTurnChangeSummary`
  - Own one map keyed by `sessionId + turnId`.
  - Track turn lifecycle, baseline snapshot metadata, changed-file records, diff provenance, and final stop reason.
- **Primary consumer:** `SessionChatTranscript`
  - Render one `TurnChangeSummaryCard` after a completed turn when the summary is non-empty.
- **Secondary consumer:** `CodeDiffView`
  - Open a dedicated diff tab when the user drills into one summarized file or the full turn patch.

## MVP Data Model

- `turnId`
  - One stable client-side turn record keyed to the `session/prompt` request lifecycle.
- `sessionId`
  - ACP session identity.
- `status`
  - `running`, `completed`, `cancelled`, `failed_to_summarize`.
- `stopReason`
  - ACP final stop reason when the turn resolves.
- `baseline`
  - Local snapshot metadata captured at turn start, such as repo root, `HEAD` commit, and whether the worktree was already dirty.
- `changedFiles`
  - One ordered list of changed file records with absolute path, change kind, optional summary stats, and provenance.
- `diffSource`
  - `git_patch`, `tool_diffs`, `paths_only`, or `none`.
- `warnings`
  - Human-readable caveats like “worktree was already dirty before this turn” or “terminal edits were inferred from git diff only”.

## Proposed Capture Flow

- **1. Start-of-turn baseline**
  - When `SessionChatState` sends `session/prompt`, start one `SessionTurnChangeSummary` draft.
  - Capture local repo metadata through a host or daemon adapter, not through ACP itself.
  - Preferred baseline:
    - repo root
    - current `HEAD`
    - pre-turn `git status --porcelain`
    - optional pre-turn patch snapshot when the worktree is already dirty
- **2. ACP event accumulation during the turn**
  - Ingest `tool_call` and `tool_call_update` events while the turn runs.
  - Collect:
    - file `locations`
    - explicit tool `diff` content
    - tool kind and status
    - terminal attachments that may imply shell-driven file edits
- **3. Finalize on prompt resolution**
  - When the original `session/prompt` request resolves, finalize the turn summary.
  - Preferred source of truth:
    - compute a local git patch from the start-of-turn baseline to the post-turn worktree
  - Fallbacks:
    - if git is unavailable or the workspace is not a repo, synthesize file summaries from ACP tool `diff` content
    - if only `locations` exist, produce a paths-only summary with an explicit warning
- **4. Render the transcript artifact**
  - Append one `TurnChangeSummaryCard` after the turn completes.
  - Show:
    - changed file count
    - short file list
    - provenance badge such as `git diff` or `tool diffs`
    - stop reason when useful
  - Let users open `CodeDiffView` for deeper review instead of forcing large inline patches into the transcript.

## Why Git Should Be Primary

- ACP tool diffs are useful but incomplete:
  - shell commands can edit files without emitting ACP `diff` content
  - one logical file change can be split across many tool updates
  - failed or cancelled tools may leave ambiguous partial edits
- A post-turn local git diff better matches what the developer actually cares about:
  - which files changed
  - what the final patch is
  - whether the final worktree matches the visible tool history
- ACP remains valuable as a fallback and as provenance when local git data is unavailable.

## Required Components

- `TurnChangeSummaryCard`
  - Inline transcript card shown after one completed turn.
- `TurnChangedFileList`
  - Compact list of changed files with per-file status and counts.
- `TurnDiffProvenanceBadge`
  - Distinguish `git diff`, `tool diffs`, and weaker fallbacks.
- `TurnSummaryOpenDiffAction`
  - Opens `CodeDiffView` for the selected turn or file.
- `TurnSummaryWarningNote`
  - Explains dirty-worktree or fallback cases without hiding them.

## Dependencies

- `SessionChatState`
  - Supplies turn boundaries, ACP updates, and final stop reasons.
- `CodeDiffView`
  - Already planned as the review surface for diff-heavy inspection.
- Local git adapter
  - Should live behind an Electrobun or daemon boundary instead of inside transcript components.
- ACP transcript item model
  - Must preserve tool-call identity and per-turn grouping so the summary can point back to the turn that produced it.

## MVP Boundaries

- Do **not** extend ACP first.
  - The app can derive the summary client-side with existing ACP data plus local git access.
- Do **not** inline full multi-file diffs directly in the transcript by default.
  - Keep the transcript compact and let `CodeDiffView` handle larger review UI.
- Do **not** attempt perfect attribution in non-git workspaces for MVP.
  - Paths-only or tool-diff summaries are acceptable with explicit provenance.

## Deferred Follow-Ons

- Custom ACP `_meta` or custom methods that let the agent send a richer turn summary when available.
- Per-tool attribution inside the final file summary.
- Turn-to-turn diff comparison when the worktree was already dirty before the prompt started.
- Non-git project support that snapshots raw file content before and after a turn.

## Open Questions

- Where should `turnId` come from when ACP itself only gives prompt lifecycle and tool ids, not a first-class turn identifier?
- How much baseline snapshotting is acceptable before the app starts paying too much overhead on every prompt turn?
- Should the transcript show a summary card for cancelled turns with partial edits by default, or only when the resulting diff is non-empty?
