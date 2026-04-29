# State Module: SessionTurnChangeSummary
- **Goal:** Show one reliable end-of-turn summary of repository file changes in `SessionChatTranscript` and let users open a dedicated turn diff in `CodeDiffView`.
- **ACP gap:** ACP gives the client `tool_call`, `tool_call_update`, tool-call `diff` content, tool-call `locations`, and the final `stopReason`, but it does **not** define a single “here are the changed files for this turn” payload or a canonical git patch emitted at turn completion.
- **Why now:** The transcript will not feel like a serious coding workspace until completed turns can show what changed, not just what the agent said or which tools ran.
- **Core decision:** This feature is git-backed or it is disabled. Do not infer final turn edits from ACP tool calls, shell commands, or transcript text.

## MVP Decision

- Turn summaries are enabled only when the daemon can resolve the session cwd to a git repository and can capture a start-of-turn git baseline.
- When git support is unavailable for a session, `SessionChatTranscript` shows no turn summary rows for that session.
- When git support is available, the daemon is the source of truth for turn summaries and turn diffs.
- ACP tool calls still matter for transcript rendering, but they are not the source of truth for changed files.

## Ownership

- **Daemon-owned turn summary service**
  - Captures git baselines at actual prompt-dispatch time.
  - Finalizes one persisted turn summary when the matching prompt request resolves.
  - Persists one on-demand git patch per completed turn for `CodeDiffView`.
- **SDK surface**
  - Exposes read APIs for turn summaries and one selected turn diff.
- **App state**
  - `SessionChatState` renders the lightweight summary list inside the transcript.
  - `CodeDiffState` loads one selected turn diff on demand when the user opens it.

## Turn Identity And Lifecycle

- `turnId`
  - Use the JSON-RPC request id of the `session/prompt` message that the daemon actually writes to the agent.
  - This id is already stable in daemon history and survives archived-session replay.
- Baseline timing
  - Capture the git baseline when the daemon dispatches the prompt from its queue, not when the app enqueues it.
  - This keeps queued prompts correct even when earlier turns change the worktree first.
- Finalization timing
  - Finalize the summary when the matching prompt response resolves with a final `stopReason`.
  - If a queued prompt is aborted before dispatch, do not create a turn summary record for it.

## Git Baseline Requirements

- A dirty worktree is a supported case for MVP.
- The daemon must persist enough start-of-turn git state to reconstruct an exact turn-only patch even when the repository was already dirty before the prompt started.
- A pre-turn patch snapshot is **not** required for MVP.
  - A patch against `HEAD` is only one delta view of the starting state and is weaker than a real pre-turn snapshot.
  - What matters is a git-addressable snapshot of the full pre-turn worktree state, including newly added untracked files.
- The baseline should capture:
  - repo root
  - `HEAD` commit or explicit null when the repository has no commit yet
  - pre-turn `git status --porcelain`
  - pre-turn full worktree snapshot sufficient to diff against the post-turn worktree

## MVP Data Model

- **Session capability**
  - `enabled`
  - optional disabled reason such as `git_unavailable` or `not_git_repository`
- **Turn summary record**
  - `sessionId`
  - `turnId`
  - `status`
    - `running`, `completed`, `failed_to_summarize`
  - `stopReason`
  - `repoRoot`
  - `startedAt`
  - `completedAt`
  - `startedDirty`
  - `changedFiles`
    - absolute path
    - change kind
    - added and removed line counts when available
  - `warnings`
    - examples: dirty worktree at turn start, git snapshot failed, diff omitted because finalization failed
- **Turn diff record**
  - `sessionId`
  - `turnId`
  - `repoRoot`
  - `patch`
  - `files`
    - normalized file-level diff data for `CodeDiffView`
  - optional post-turn verification state for future turn-scoped undo and conflict reporting

## Capture Flow

- **1. Prompt dispatch**
  - When the daemon dequeues one `session/prompt` request and is about to write it to the agent, capture the git baseline for that `turnId`.
  - If baseline capture fails, mark turn-summary support unavailable for that session or mark the turn as `failed_to_summarize` when the failure is turn-local.
- **2. Turn execution**
  - The daemon continues to persist normal ACP history.
  - The app keeps using ACP updates for transcript rows, but not for changed-file attribution.
- **3. Prompt resolution**
  - When the matching prompt response arrives, capture one post-turn worktree snapshot.
  - Compute the turn-only git patch from the start-of-turn snapshot to the post-turn snapshot.
  - Persist a lightweight summary plus an on-demand diff payload keyed by `sessionId + turnId`.
  - Persist enough post-turn verification data to support a future daemon-owned "undo changes made in this turn" method that best-effort reverse-applies the turn patch and surfaces conflicts.
- **4. Transcript rendering**
  - `SessionChatTranscript` appends one `TurnChangeSummaryCard` after a turn only when the finalized turn diff is non-empty.
  - Cancelled turns still show a summary card when they produced a non-empty git diff.
  - Turns with no resulting diff render no summary card.
- **5. Diff drill-down**
  - `TurnSummaryOpenDiffAction` opens `CodeDiffView` backed by the persisted turn diff, not by recomputing from tool calls.

## Required Components

- `TurnChangeSummaryCard`
  - Compact end-of-turn transcript artifact with file count, short file list, and stop state when useful.
- `TurnChangedFileList`
  - Compact changed-file list with per-file status and counts.
- `TurnSummaryOpenDiffAction`
  - Opens the dedicated turn diff in `CodeDiffView`.
- `TurnSummaryWarningNote`
  - Explains dirty-worktree and summarization-failure cases without pretending the data is cleaner than it is.

## Shared Dependencies

- **`core/schema/`**
  - Add durable types for session turn-summary capability, turn summary records, and one turn-diff payload.
- **`core/daemon/`**
  - Persist turn summary metadata separately from ACP history.
  - Add daemon-owned git snapshot and diff helpers that run against the session repository root.
  - Add IPC requests to list turn summaries for one session and fetch one selected turn diff.
  - Store enough pre-turn and post-turn state to support future turn-scoped undo without re-inferring edits from ACP history.
  - Reserve a future daemon method that reverse-applies one finalized turn patch on a best-effort basis and returns conflict details for anything that no longer applies cleanly.
- **`core/sdk/`**
  - Mirror those daemon IPC methods through `sdk.session`.
- **`app/`**
  - Add summary queries alongside session history queries.
  - Invalidate turn-summary queries when the live session observes the final response for one prompt turn.

## MVP Boundaries

- Do **not** extend ACP for this feature first.
- Do **not** synthesize changed-file summaries from ACP tool diffs or file locations.
- Do **not** show turn summaries for non-git sessions.
- Do **not** inline the full multi-file patch in the transcript.
- Do **not** attempt to summarize edits outside the repository root or ignored files for MVP.

## Deferred Follow-Ons

- Per-tool attribution inside one finalized turn summary.
- Richer session-level UX when turn summaries are disabled for a non-git session.
- Replay affordances that compare consecutive turn diffs inside the transcript itself.
- Agent-provided `_meta` hints that enrich the summary without replacing the daemon git diff.
- One daemon-owned "undo changes made in this turn" method built on the persisted turn snapshot and verification data.
  - Reverse-apply the finalized turn patch on a best-effort basis.
  - Surface conflicts and partial-apply results instead of requiring an exact clean-match revert.

## Implementation Readiness

- The main old ambiguities are resolved:
  - `turnId` comes from the dispatched prompt request id.
  - Baselines are captured at daemon dispatch time, not app enqueue time.
  - Dirty-worktree support requires a full pre-turn git snapshot, not a patch snapshot against `HEAD`.
  - Git missing means summaries are disabled, not replaced with weaker fallbacks.
