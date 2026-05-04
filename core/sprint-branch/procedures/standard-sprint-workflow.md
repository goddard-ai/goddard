# Standard Sprint Workflow

A typical sprint moves through this sequence:

1. Create or restore sprint branch state with `init` or `reset-state`.
2. Begin the next planned task with `start`.
3. Complete implementation work on the assigned branch.
4. Mark the task ready for review with `finish`.
5. Use `view`, `diff`, `checkout`, or `sync` to support human review.
6. Promote accepted work with `approve`.
7. Repeat `start`, `finish`, and `approve` until all tasks are approved.
8. Prepare the completed sprint for merge with `finalize`.
9. Merge it into the target branch with `land`.
10. Remove the sprint-specific branches, review worktrees, and private state
    with `cleanup`.

When work-ahead is active, `feedback` and `resume` handle interruptions:
`feedback` makes the review branch available for human-requested changes, and
`resume` returns to the dependent `next` work afterward.

`doctor` is the recovery entry point whenever the state is unclear. It explains
what is inconsistent and names the next safe command when one can be determined.
