# Review Sync Model

- **Core idea**
  - `review-sync` lets a human review an agent-owned branch from a separate local worktree.
  - The agent keeps ownership of the agent branch.
  - The human works on a disposable review branch derived from the agent branch.
  - Sync turns review-side changes into patches and keeps the review worktree
    refreshed from the agent's current content.

- **Branch and worktree roles**
  - `Agent Branch`
    - The branch the agent is actively changing.
    - It must be checked out in a separate agent worktree for normal start and
      one-shot sync operations.
  - `Agent Worktree`
    - The worktree that owns the agent branch during a review-sync session.
    - Accepted human patches are applied here.
  - `Review Branch`
    - The human-facing branch named by adding `review-sync/` before the full agent branch name.
    - For example, `codex/example` reviews through `review-sync/codex/example`.
    - It is disposable: its content matters, but humans should not rely on its
      commit history as the durable review record.
  - `Review Worktree`
    - The separate local worktree a human opens in an editor.
    - It is where the review branch is checked out and refreshed.

- **Session ownership**
  - A review-sync session binds:
    - One agent branch.
    - One derived review branch.
    - One agent worktree.
    - One review worktree.
    - One shared Git repository.
  - Commands that do not name an agent branch infer the session from the current
    worktree or checked-out branch.
  - If more than one saved session matches the current worktree, commands refuse to guess.
    - The safe recovery is to run from a worktree that clearly belongs to one
      session, name the agent branch where supported, or remove stale saved
      sessions only after checking their saved patches.

- **Rendered snapshot**
  - A rendered snapshot is the review branch content from the last successful refresh.
  - It is the baseline used to decide whether the human changed the review worktree.
  - It is not a user-facing branch role; it exists so the next sync can
    distinguish review edits from agent edits.

- **Human patches**
  - A human patch is the visible difference between the last rendered snapshot
    and the current review worktree.
  - Human patches include tracked changes and untracked files that Git does not ignore.
  - Ignored files are not synchronized.
  - Human commits on the review branch are treated as review content:
    - The resulting file changes can be accepted.
    - The review branch history itself is not preserved as the durable record.

- **Patch outcomes**
  - `ok`
    - The session command completed without a rejected human patch.
    - If the human patch applied cleanly, it is saved as accepted and applied to the agent worktree.
    - If there was no human patch, the review worktree is refreshed from the agent content.
  - `rejected-human-patch`
    - The human patch could not apply cleanly to the agent worktree.
    - The patch is saved for recovery.
    - The agent worktree is not changed by that patch.
    - The review worktree is refreshed back to the agent content so review can
      continue from a coherent state.
  - `paused`
    - The session is intentionally blocked from sync mutations.
    - Existing worktree edits are not discarded by pausing.
  - `error`
    - The command refused or failed before completing its intended transition.

- **Guardrails**
  - Agent and review worktrees must belong to the same Git repository.
  - The agent branch must not itself be a review branch.
  - The derived review branch must not be checked out in an unrelated worktree.
  - Sync mutations require the recorded agent worktree to be on the recorded agent branch.
  - Sync mutations require the recorded review worktree to be on the derived review branch.
  - In-progress Git operations that make branch or patch movement ambiguous are blocked.
  - Paused sessions block sync mutations until resumed.
  - Review branch checkout or reset operations protect local review edits
    instead of silently discarding them.

- **Human and agent ownership**
  - The agent branch remains the agent-owned branch.
  - The review branch is the human review surface, but it is still managed by `review-sync`.
  - Human review edits are durable when saved as accepted or rejected patches.
  - Agents and humans may run commands from either recorded worktree after a
    session exists, except setup flows that need a review worktree to prepare
    the review branch.
