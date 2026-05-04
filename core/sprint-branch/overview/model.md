# Sprint Branch Model

- **Core idea**
  - `sprint-branch` manages a sprint as a small rolling branch system.
  - The system has:
    - One human review boundary.
    - One optional work-ahead slot.

- **Branch roles**
  - `approved`
    - The last human-approved sprint content.
    - The baseline for review diffs and future sprint work.
  - `review`
    - The content currently being reviewed or prepared for review.
    - The branch humans inspect before approval.
  - `next`
    - Optional work-ahead content.
    - Depends on `review` and must be reconciled after review changes.

- **Task source**
  - Task markdown files in `sprints/<name>/` define sprint task order.
  - Sprint state records:
    - Which tasks are approved.
    - Which task is on `review`.
    - Which task is on `next`.
    - Which active tasks are finished but not yet approved.

- **Task states**
  - `planned`
    - Present in the sprint folder.
    - Not assigned to a branch.
  - `review`
    - Assigned to the review branch.
  - `next`
    - Assigned to the work-ahead branch.
  - `finished-unreviewed`
    - Marked complete by the agent.
    - Ready for human review.
    - Not yet approved.
  - `approved`
    - Accepted into the approved branch.
    - No longer active.

- **Sprint resolution**
  - Most commands first resolve the active sprint.
  - Shared selectors, prompted ordering, parked sprint handling, and activity
    tracking are defined in [Sprint selection](./sprint-selection.md).

- **Planning and output**
  - Many commands support `--json` for machine-readable output.
  - Mutating workflow commands support `--dry-run`.
    - Dry runs report the intended transition.
    - Dry runs do not change branches, task state, activity timestamps, or
      working tree files.
  - Human landing commands support JSON only for dry-run inspection.
    - Real landing and cleanup require interactive confirmation.

- **Clean-working-tree guardrails**
  - Commands that switch, move, rebase, or finalize branches usually require a
    clean working tree.
  - The guardrail exists so unrelated local edits are not stranded inside a
    sprint transition.
  - Commands with narrower requirements include:
    - Read-only reporting commands.
    - Private sprint metadata updates.
    - Commands that intentionally preserve interrupted work.

- **Interrupted transitions**
  - Some commands may stop at a conflict boundary and record recovery state.
  - While recovery state exists:
    - Other mutating commands are blocked.
    - The same command is retried after the user resolves the conflict.
    - `doctor` explains the recorded recovery state and the next safe command.
