# Review Sync Glossary

- `Agent Worktree`
  - The Git worktree where the agent branch is checked out and where accepted human patches are applied.
- `Review Worktree`
  - The separate local Git worktree a human opens in their editor.
- `Review Branch`
  - The disposable branch derived as `<agent-branch>--review` and checked out in the review worktree.
- `Rendered Snapshot`
  - The synthetic snapshot commit last written into the review branch.
- `Human Patch`
  - The binary Git diff from the rendered snapshot to the review worktree's current snapshot.
- `Accepted Patch`
  - A human patch that applied cleanly to the agent worktree.
- `Rejected Patch`
  - A human patch that could not apply cleanly and was preserved without mutating the agent worktree.
