# Standard Review Workflow

- **Setup**
  - Start with the agent branch checked out in an agent worktree.
  - Open a separate local worktree for human review.
  - Run `start <agent-branch>` from the review worktree, or run
    `watch <agent-branch>` to start and keep watching in one command.
  - The review worktree is checked out to the derived review branch.
  - The first refresh makes the review worktree show the agent branch content.

- **Agent-to-review flow**
  - When the agent changes files on the agent branch, the next sync refreshes the review branch.
  - Agent changes can be committed or uncommitted.
  - Non-ignored untracked files are included.
  - The review worktree is reset to the latest synchronized agent content.

- **Human-to-agent flow**
  - When the human edits the review worktree, the next sync treats those changes as a human patch.
  - If the patch applies cleanly:
    - The patch is saved as accepted.
    - The same file changes appear in the agent worktree.
    - The review branch is refreshed from the resulting agent content.
  - If the patch does not apply cleanly:
    - The patch is saved as rejected.
    - The agent worktree is left unchanged by the rejected patch.
    - The review worktree is refreshed from the agent content.
    - A human or agent can inspect the rejected patch and decide how to rework it.

- **Watch mode**
  - `watch` keeps the review loop active by syncing after agent, review, or agent-branch changes.
  - With an explicit agent branch, `watch` can start or reuse the session before watching.
  - If the agent branch is temporarily not checked out, `watch` waits instead of
    failing the whole review loop.
  - While waiting, `watch` may show the review branch from the agent branch ref
    when doing so will not discard human work.
  - When stopped, `watch` pauses the session and tries to restore the review
    worktree to the branch that was active when watching began.

- **Pause and resume**
  - Use `pause` when sync mutations should temporarily stop.
  - A paused session keeps its saved relationship between the agent worktree and review worktree.
  - Use `resume` to allow future sync mutations again.
  - `resume` does not apply pending review edits by itself; run `sync` or `watch` afterward.

- **Recovery**
  - Use `status` to orient before taking recovery action.
  - If a human patch is rejected:
    - Inspect the saved rejected patch.
    - Reapply or rewrite the intended change against the current agent content.
    - Run `sync` again when the review worktree contains the next intended patch.
  - If the agent worktree is on another branch:
    - One-shot `sync` refuses to mutate.
    - `watch` waits for the recorded agent branch to return.
  - If review-side edits block a safe refresh:
    - `watch` leaves those edits in place and reports the blocked refresh.
    - The next safe action is to save, sync, or intentionally discard that review-side work.
  - If session inference is ambiguous:
    - Run from a clearer worktree or use an explicit agent branch where supported.
    - Remove stale saved sessions only after checking whether they contain
      accepted or rejected patches that matter.
