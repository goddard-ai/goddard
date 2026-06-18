# Sprint Branch Commands

> Sprint-branch commands manage a local stack of agent task branches from setup through review, approval, landing, and cleanup. This index groups commands by the workflow question they answer.

- **Inspection**
  - [`status`](./status.md)
    - Inspect sprint branch state and the next safe action.
  - [`diff`](./diff.md)
    - Show the review delta against approved work.
  - [`view`](./view.md)
    - Print the approval packet for a finished task.
  - [`doctor`](./doctor.md)
    - Diagnose inconsistent sprint state and recovery direction.
  - [`list`](./list.md)
    - List known active or parked sprints.

- **Human review and landing**
  - [`checkout`](./checkout.md)
    - Inspect a review branch without taking branch ownership.
  - [`sync`](./sync.md)
    - Watch the active review branch through review-sync.
  - [`stop-sync`](./stop-sync.md)
    - Ask running sync commands from the same working directory to stop.
  - [`land`](./land.md)
    - Fast-forward a target branch to finalized sprint work.
  - [`cleanup`](./cleanup.md)
    - Remove landed sprint branches, worktree checkouts, and state.

- **Setup, recovery, and visibility**
  - [`init`](./init.md)
    - Create the branch scaffold and initial sprint state.
  - [`reset-state`](./reset-state.md)
    - Rebuild sprint state around a selected next task.
  - [`restore-sprint`](./restore-sprint.md)
    - Restore an accidentally removed sprint folder from local backup.
  - [`park`](./park.md)
    - Hide a sprint from default active selection.
  - [`unpark`](./unpark.md)
    - Restore a parked sprint to default active selection.

- **Agent workflow**
  - [`start`](./start.md)
    - Assign the next planned task to a rolling branch.
  - [`finish`](./finish.md)
    - Mark an active task ready for human review.
  - [`feedback`](./feedback.md)
    - Interrupt work-ahead and return to review.
  - [`resume`](./resume.md)
    - Return to interrupted or dependent next work.
  - [`approve`](./approve.md)
    - Promote reviewed work into approved and roll the queue forward.
  - [`rebase`](./rebase.md)
    - Move the sprint branch stack onto a new target ref.
  - [`finalize`](./finalize.md)
    - Prepare fully approved sprint work for human landing.
