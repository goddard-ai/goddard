# `sprint-branch finalize [--override-base <ref>]`

- **Question it answers**
  - Is the fully approved sprint ready for the human's final merge?

- **What it does**
  - Prepares the completed review branch for landing.
  - Brings completed review content onto the sprint's recorded base.
  - Updates the approved boundary to match review.
  - Leaves the review branch as the branch humans land from.

- **Inputs**
  - `--override-base <ref>` is available for recovery when the recorded base is
    not the target humans intend to land onto.

- **What it changes**
  - Review branch content.
  - Approved branch boundary.
  - Sprint base state.

- **Guardrails**
  - No review task may be active.
  - No next task may be active.
  - No finished-unreviewed task may remain.
  - Review and approved must represent the same approved content.
  - `next` must not contain different work.
  - The working tree must be clean.
  - The base ref must resolve.
  - If a finalize conflict occurs:
    - Sprint state remains at the pre-finalize boundary.
    - `finalize` must be retried after conflict resolution.

- **Why it exists**
  - Approval finishes the task queue.
  - Finalization prepares the whole approved sprint for a clean human landing.
