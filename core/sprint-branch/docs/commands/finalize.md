# `sprint-branch finalize [--override-base <ref>] [--ignore-next-branch]`

The sprint-branch finalize [--override-base <ref>] [--ignore-next-branch] command is part of the local sprint-branch workflow. This page explains when to use it, what sprint state or branches it may change, and what guardrails apply.

## Question it answers

- Is the fully approved sprint ready for the human's final merge?

## Inputs and selection

- Uses [standard sprint selection](../sprint-selection.md).
- `--override-base <ref>` is available for recovery when the recorded base is
    not the target humans intend to land onto.
- `--ignore-next-branch` is available for recovery when an obsolete dormant
    `next` branch differs from finalized `review`.
  - After a successful finalization, the accepted `review` and `next`
      commits are recorded so later finalized-sprint commands keep treating
      that same divergence as a warning.

## What it does

- Prepares the completed review branch for landing.
- Brings completed review content onto the sprint's recorded base.
- Updates the approved boundary to match review.
- Leaves the review branch as the branch humans land from.

## What it changes

- Review branch content.
- Approved branch boundary.
- Sprint base state.
- Checkout state:
  - Leaves `review` checked out after success.

## Guardrails

- No review task may be active.
- No next task may be active.
- No finished-unreviewed task may remain.
- Review and approved must represent the same approved content.
- `next` must not contain different work.
  - `--ignore-next-branch` downgrades only this check to a warning.
  - The downgrade is preserved only for the exact `review` and `next`
      commits accepted during successful finalization.
- The working tree must be clean.
- The base ref must resolve.
- If a finalize conflict occurs:
  - Sprint state remains at the pre-finalize boundary.
  - `finalize` must be retried after conflict resolution.
- On retry, `--override-base` can replace the base used for the finalization
    attempt.

## Dry run

- Reports how the completed sprint would be prepared for landing.
- Does not move branches.
- Does not switch checkout state.
- Does not update sprint state.

## Why it exists

- Approval finishes the task queue.
- Finalization prepares the whole approved sprint for a clean human landing.
