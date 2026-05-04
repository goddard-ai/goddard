# `review-sync sync`

- **Question it answers**
  - What happens when the current agent and review worktrees need to converge once?

- **Session selection**
  - The session is inferred from the current worktree or checked-out branch.
  - The command may run from either the recorded agent worktree or the recorded review worktree.
  - If multiple saved sessions match, the command refuses to choose one.

- **What it does**
  - Detects whether the review worktree has changed since the last rendered snapshot.
  - Applies a clean human patch to the agent worktree and saves it as accepted.
  - Saves a conflicting human patch as rejected without applying it to the agent worktree.
  - Captures the latest agent content after any accepted patch.
  - Refreshes the review branch to that latest agent content.

- **What it changes**
  - The agent worktree when a human patch is accepted.
  - The review branch and review worktree after every completed sync.
  - The saved accepted or rejected patch inventory when a human patch exists.
  - The session's last-sync outcome.

- **What it never changes**
  - It does not mutate a paused session.
  - It does not apply a rejected human patch to the agent worktree.
  - It does not preserve review branch commit history as the durable record.
  - It does not synchronize ignored files.

- **Guardrails**
  - The recorded agent worktree must be on the recorded agent branch.
  - The recorded review worktree must be on the derived review branch.
  - Both worktrees must still belong to the recorded Git repository.
  - In-progress Git operations that make patch or branch movement unsafe are refused.
  - A paused session returns a paused result without changing files.

- **Recovery behavior**
  - On `rejected-human-patch`, inspect the saved rejected patch and rework the
    change against the current agent content.
  - After manual recovery, run `sync` again from either recorded worktree.
  - If the agent worktree is on another branch, return it to the recorded agent
    branch before retrying one-shot sync.
