# Sprint Branch Model

`sprint-branch` manages a sprint as a small rolling branch system with one
human review boundary and one optional work-ahead slot.

- `approved` is the last human-approved sprint content.
- `review` is the content currently being reviewed or prepared for review.
- `next` is optional work-ahead content that depends on `review`.

Task markdown files in `sprints/<name>/` define the sprint task order. The
command state records which tasks are approved, which task is on `review`, which
task is on `next`, and which active tasks have been marked finished but not yet
approved.

The normal task states are:

- `planned`: present in the sprint folder but not assigned to a branch.
- `review`: assigned to the review branch.
- `next`: assigned to the work-ahead branch.
- `finished-unreviewed`: marked complete by the agent and ready for human
  review, but not yet approved.
- `approved`: accepted into the approved branch and no longer active.

Most commands first resolve the active sprint. A sprint can be supplied
explicitly, inferred from the current sprint branch, inferred from a working
directory under `sprints/<name>`, or selected interactively from active sprint
state. Non-interactive callers must pass a sprint when it cannot be inferred
from strong local context.

Many commands support `--json` for machine-readable output. Mutating workflow
commands support `--dry-run`, which reports the intended transition without
changing branches, task state, or working tree files. Human landing commands
support JSON only for dry-run inspection because real landing and cleanup
require an interactive confirmation.

The clean-working-tree requirements are intentional. Commands that switch,
move, rebase, or finalize branches usually require a clean working tree so
unrelated local edits are not stranded inside a sprint transition. Commands that
only report state, update private sprint metadata, or intentionally preserve
interrupted work have narrower requirements.
