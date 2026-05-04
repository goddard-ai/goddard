# `review-sync start <agent-branch>`

- **Question it answers**
  - How does a separate review worktree become the human review surface for an agent branch?

- **Inputs and selection**
  - `<agent-branch>` names the agent branch that is already checked out in another worktree.
  - In an interactive terminal, omitting the branch can open a prompt for
    eligible checked-out agent branches.
  - Non-interactive callers must provide the agent branch.

- **What it does**
  - Creates or reuses a review-sync session for the agent branch and current review worktree.
  - Derives the review branch by prefixing the full agent branch with `review-sync/`.
  - Checks out the derived review branch in the review worktree.
  - Reactivates the session if it was paused.
  - Runs an initial sync so the review branch is aligned with the agent content.

- **What it changes**
  - Review worktree checkout state.
  - Review branch content.
  - Review-sync session state.
  - If the review branch already contained human work, a clean human patch may
    be accepted into the agent worktree.
  - If that existing human work conflicts, the patch is saved as rejected and
    the agent worktree is left unchanged by it.

- **Guardrails**
  - The command is intended to run from the review worktree.
  - The agent branch must be checked out in another worktree.
  - The agent branch cannot already be a `review-sync/` branch.
  - The agent and review worktrees must be in the same Git repository.
  - The derived review branch must not be checked out in another unrelated worktree.
  - The review worktree must be clean before `start` switches it to the review
    branch or refreshes without a known baseline.
  - In-progress Git operations that make branch movement unsafe are refused.

- **Common next actions**
  - Run `status` to confirm the session and patch counts.
  - Run `sync` after either side changes.
  - Run `watch <agent-branch>` instead of repeated manual syncs when reviewing live agent work.
