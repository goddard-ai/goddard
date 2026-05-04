# Standard Sprint Workflow

- **Setup**
  - Create the sprint branch scaffold with `init`.
  - Restore or realign private sprint state with `reset-state` when a sprint
    plan has changed or state needs recovery.

- **Task loop**
  - Start the next planned task with `start`.
    - The first active task goes to `review`.
    - Work-ahead goes to `next` only while `review` is occupied.
  - Complete implementation work on the assigned branch.
  - Mark the task ready for review with `finish`.
    - The task becomes `finished-unreviewed`.
    - The task is not approved yet.
  - Support human review with:
    - `view` for the approval packet.
    - `diff` for the review delta.
    - `checkout` for a detached review snapshot.
    - `sync` for the review-sync watch workflow.
  - Promote accepted review work with `approve`.
    - The approved boundary advances.
    - Existing work-ahead rolls forward into review.
  - Repeat until all sprint tasks are approved.

- **Work-ahead interruptions**
  - Use `feedback` when human feedback needs the review branch while the agent
    is working ahead.
    - Dirty `next` work is preserved as interrupted sprint work.
    - The working context returns to `review`.
  - Use `resume` after review feedback is handled.
    - Dependent `next` work is brought forward.
    - Preserved interrupted work is restored when applicable.

- **Completion**
  - Use `finalize` when no review task, next task, or finished-unreviewed task
    remains.
    - The fully approved sprint is prepared for human landing.
  - Use `land` to fast-forward the target branch to the finalized sprint work.
    - This is a human command.
    - Real execution requires interactive confirmation.
  - Use `cleanup` after landing.
    - Landed sprint branches are removed.
    - Clean associated review worktrees are removed.
    - Private sprint state is removed.

- **Recovery**
  - Use `doctor` whenever state is unclear.
    - It explains inconsistent state.
    - It names the next safe command when one can be determined.
