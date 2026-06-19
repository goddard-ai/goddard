# `sprint-branch rebase <target>`

> The sprint-branch rebase <target> command is part of the local sprint-branch workflow. This page explains when to use it, what sprint state or branches it may change, and what guardrails apply.

## Question it answers

- How can the whole sprint branch stack move onto a newer base?

## Inputs and selection

- Uses [standard sprint selection](../sprint-selection.md).
- `<target>` is the ref that should become the sprint's new base.

## What it does

- Moves recorded sprint branches onto a target ref.
- Preserves the rolling relationship between:
  - `approved`.
  - `review`.
  - `next`.
- Records the new base only after every relevant sprint branch has moved
    successfully.

## What it changes

- Recorded sprint branch commits.
- Sprint base state after all branch movement succeeds.
- Returns the checkout to the branch that was current before the rebase when
    possible.

## Guardrails

- The target must exist.
- The target must not be one of the sprint branches.
- The target must share history with the current approved branch.
- The working tree must be clean.
- No interrupted sprint stashes may be active.
- The recorded branch stack must already be coherent:
  - `review` must descend from `approved`.
  - `next`, when present, must descend from `review`.
    - A dormant `next` divergence that was explicitly accepted by a
        successful `finalize --ignore-next-branch` is skipped instead.
- If a conflict interrupts the transition:
  - The prior base remains recorded.
  - `rebase` can be retried after conflict resolution.

## Dry run

- Reports the branch stack movement that would occur.
- Does not move branches.
- Does not switch checkout state.
- Does not update the recorded base.

## Why it exists

- It keeps long-running sprint work current with the target branch.
- It avoids collapsing the review boundary or losing the work-ahead
    relationship.
