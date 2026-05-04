# `sprint-branch` Command Map

This document describes what each `sprint-branch` command supports at a
conceptual level. It is written for agents and humans, including people who need
to understand the sprint review process without reading source code.

It intentionally avoids implementation details. It describes command outcomes,
guardrails, and the procedural shape of the workflow, not the internal files,
schemas, locks, or command implementation.

## Shared Model

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

## Workflow Shape

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

## Command Index

| Command | Primary audience | Mutates state or branches | Purpose |
| --- | --- | --- | --- |
| `status` | Agents and humans | No | Inspect sprint branch state and the next safe action. |
| `diff` | Agents and humans | No | Show the review delta against approved work. |
| `view` | Humans, agents preparing review | No | Print the approval packet for a finished task. |
| `sync` | Humans reviewing agent work | Delegates to review sync | Watch the active review branch through `review-sync`. |
| `doctor` | Agents and humans | No | Diagnose inconsistent sprint state and recovery direction. |
| `list` | Agents and humans | No | List known active or parked sprints. |
| `checkout` | Humans | Checks out a detached snapshot | Inspect a review branch without taking branch ownership. |
| `land` | Humans | Yes | Fast-forward a target branch to finalized sprint work. |
| `cleanup` | Humans | Yes | Remove landed sprint branches, review worktrees, and state. |
| `init` | Agents setting up a sprint | Yes | Create the branch scaffold and initial sprint state. |
| `reset-state` | Agents recovering or replanning | Yes, state only | Rebuild private sprint state around a selected next task. |
| `park` | Agents and humans | Yes, state only | Hide a sprint from default active selection. |
| `unpark` | Agents and humans | Yes, state only | Restore a parked sprint to default active selection. |
| `start` | Agents | Yes | Assign the next planned task to `review` or `next`. |
| `finish` | Agents | Yes, state only | Mark an active task ready for human review. |
| `feedback` | Agents | Yes | Interrupt work-ahead and return to `review`. |
| `resume` | Agents | Yes | Return to interrupted or dependent `next` work. |
| `approve` | Agents after human approval | Yes | Promote reviewed work into `approved` and roll the queue forward. |
| `rebase` | Agents | Yes | Move the whole sprint branch stack onto a new target ref. |
| `finalize` | Agents | Yes | Prepare fully approved sprint work for human landing. |

## Inspection Commands

### `sprint-branch status`

`status` answers "what is the current sprint branch state?"

It reports the resolved sprint, current branch, sprint visibility, branch roles,
branch existence, branch ancestry, working tree cleanliness, task assignments,
task queue, finished-but-unreviewed tasks, diagnostics, and the next safe
command when one is known.

`status` does not change branches, task state, review state, or working tree
files. It can still fail when the sprint cannot be inferred or the recorded
state is too invalid to read.

Why it matters: agents use `status` as the basic orientation command before
choosing a workflow action. Humans can use it to understand whether a sprint is
ready for review, blocked, or safe to continue.

### `sprint-branch diff`

`diff` answers "what changed between the approved boundary and the review
boundary?"

It shows the review branch delta against approved work. `--name-only` narrows
the output to changed paths. `--stat` shows a summary of changed files and line
counts.

`diff` requires the approved and review branches to exist and the review branch
to be based on approved work. It does not change anything.

Why it matters: this is the sprint-level review comparison. It keeps review
focused on unapproved work instead of the full repository history.

### `sprint-branch view [--task <task>]`

`view` answers "what should a human read to approve this task?"

It prints the concise approval view for a finished task. By default, it uses the
task currently assigned to `review`; `--task` selects a specific task by task
file stem. The view includes the task identity, review and approved branch names,
the recommended diff command, and the task's Review Report.

The selected task must exist in sprint state and be marked
`finished-unreviewed`. Its task markdown must contain a complete Review Report
with these sections:

- `Plain-English Summary`
- `How To Verify Without Reading Code`
- `Agent Verification`
- `Approval Questions`
- `Known Limits`

`view` does not change anything.

Why it matters: it creates a durable review handoff. The human does not need to
infer intent from code or branch names alone.

### `sprint-branch sync`

`sync` answers "how do I watch the active sprint review branch through the
review-sync workflow?"

It resolves the active sprint, validates the sprint status, and starts a
`review-sync` watch session for the sprint's review branch. `review-sync` is the
separate Git-only review workflow that keeps a disposable review branch aligned
with the agent-owned branch while preserving human review edits.

If sprint status is invalid, `sync` reports the sprint diagnostics instead of
starting the watch. While running, it surfaces `review-sync` results and exits
with the review-sync outcome.

Why it matters: it lets humans review the current sprint review branch from a
review worktree while agents continue to own the sprint workflow branch.

### `sprint-branch doctor`

`doctor` answers "what is unsafe or inconsistent, and what should happen next?"

It starts from the same broad inspection surface as `status`, then applies
deeper consistency checks. It looks for states such as missing branch roles,
unexpected branch ancestry, unrecorded branch work, task ordering problems,
duplicate task assignments, incomplete Review Reports for finished tasks,
recorded conflicts, active Git operations, stale recovery state, active sprint
stashes, and extra sprint namespace branches.

`doctor` does not repair state by itself. Its output explains the problem and,
when possible, names the next safe command, such as retrying `resume`,
`approve`, `rebase`, or `finalize` after a conflict has been resolved.

Why it matters: it is the first command to run when the workflow no longer feels
linear. It keeps agents from guessing which branch or task state is safe to
mutate.

### `sprint-branch list [--all]`

`list` answers "which sprint branch states are known?"

By default, it lists active sprints. `--all` includes parked sprints. Each entry
shows the sprint name, visibility, and review branch.

`list` does not infer a current sprint and does not change anything. Unreadable
sprint state appears as diagnostics rather than stopping the entire listing.

Why it matters: it is the low-risk discovery command for choosing a sprint when
the current directory or branch does not identify one.

## Human Review And Landing Commands

### `sprint-branch checkout [name]`

`checkout` answers "how can a human inspect the sprint review branch without
taking over the live branch?"

It checks out the selected sprint's review branch as a detached snapshot. The
review branch remains agent-owned; the human is looking at a commit snapshot,
not moving the branch forward by working on it directly.

If `name` is omitted, the sprint can be inferred from the current sprint branch
or `sprints/<name>` working directory. In an interactive terminal, the command
can prompt for an active sprint. Non-interactive callers must provide `name`
when no strong context exists.

`checkout` requires a clean working tree before switching snapshots. It supports
`--dry-run` to show which sprint and review branch would be checked out.

Why it matters: human review needs a stable, inspectable snapshot, while sprint
branches remain controlled by the workflow commands.

### `sprint-branch land <target> [name]`

`land` answers "how does finalized sprint work enter the target branch?"

It fast-forwards a human-selected target branch, such as `main`, to the finalized
sprint review content. It is a human landing command and requires interactive
confirmation for real execution. `--dry-run --json` is supported for
non-interactive inspection.

The sprint must be finalized before it can land:

- no task may still be assigned to `review` or `next`;
- no task may remain `finished-unreviewed`;
- no sprint conflict may be recorded;
- no interrupted sprint stash may remain active;
- `review` and `approved` must represent the same finalized content;
- `next`, if present, must not contain different work;
- the target branch must exist and must not itself be a sprint branch;
- the target must be able to fast-forward to the finalized review content.

`land` does not delete sprint branches or sprint state. It only moves the target
branch when the finalized review content is ready to become target-branch
content.

Why it matters: it separates final human merge authority from agent workflow
approval. Agents can prepare a sprint, but landing remains a deliberate human
operation.

### `sprint-branch cleanup <target> [name]`

`cleanup` answers "how do we remove sprint-specific local artifacts after
landing?"

It removes landed sprint branches, clean associated review worktrees, and the
private sprint state for the selected sprint. It is a human command and requires
interactive confirmation for real execution. `--dry-run --json` is supported for
non-interactive inspection.

The target branch must contain the finalized review commit before cleanup can
proceed. Cleanup also requires the sprint to be finalized, the working tree to
be clean, and the current branch not to be one of the branches that would be
deleted.

Why it matters: cleanup is intentionally separate from landing. A sprint can be
landed and inspected before local sprint branches and review worktrees are
removed.

## Setup, Recovery, And Visibility Commands

### `sprint-branch init [--base <ref>]`

`init` answers "how does a sprint become branch-managed?"

It creates the initial sprint branch scaffold and records initial sprint state.
The base ref defaults to `main` unless `--base` is provided. The sprint folder
must already exist, the base ref must resolve, and the expected sprint branches
and state must not already exist.

After initialization, `approved` and `review` start at the base boundary, no task
is active, and the sprint is active for default selection.

`init` does not require a clean working tree because it creates the scaffold
rather than moving existing work between sprint branches.

Why it matters: it defines the review boundary before any task starts. Starting
from a known approved base is what makes later review diffs meaningful.

### `sprint-branch reset-state [--task <task>] [--base <ref>] [--force]`

`reset-state` answers "how can private sprint state be recreated after a sprint
plan is reworked or state is missing?"

It rewrites only the private sprint state. It does not move sprint branches. The
selected task becomes the next valid `start` target. If no task is selected, the
first task in sprint task-file order becomes next. Tasks before the selected
task are recorded as already approved.

The command requires a clean working tree, an existing sprint folder, at least
one task file, a resolvable base ref, and existing approved and review branches.
It also checks whether current branch contents appear to contain active or
unrecorded sprint work. `--force` converts certain active-work and branch-drift
blockers into warnings, so it should only be used after preserving or
intentionally discarding the work those warnings describe.

Why it matters: it gives agents a recovery path when the human-authored sprint
plan changes, without pretending to safely move branch content that it is not
moving.

### `sprint-branch park`

`park` answers "how can this sprint stay recorded but stop appearing in default
active selection?"

It marks the sprint as parked. Parked sprints still retain their branches and
state, and can still be selected explicitly. They are hidden from default active
selection and from `list` unless `--all` is used.

`park` does not move branches or require a clean working tree.

Why it matters: parking keeps inactive or paused sprint state available without
making agents choose among stale active candidates.

### `sprint-branch unpark`

`unpark` answers "how can a parked sprint become active again?"

It marks the sprint as active for default selection. It does not move branches,
change task assignments, or require a clean working tree.

Why it matters: it restores a paused sprint to the normal command-selection
flow without reconstructing state.

## Agent Workflow Commands

### `sprint-branch start --task <task>`

`start` answers "which branch should the agent use for the next task?"

It assigns the requested task to the correct rolling branch:

- If no task is on `review`, the requested task starts on `review`.
- If the requested task is already on `review`, the command continues that
  review task.
- If `review` is occupied and `next` is empty, the requested task starts as
  work-ahead on `next`.
- If the requested task is already on `next`, the command continues that next
  task.

The requested task must be the next unassigned task in sprint task-file order.
The workflow supports at most two active tasks at once: one on `review` and one
on `next`. Starting a third task is blocked until review, feedback, resume, or
approval moves the queue forward.

`start` requires a clean working tree because it may switch to a sprint branch
and update which branch represents the requested task.

Why it matters: it keeps sprint work ordered and prevents agents from inventing
extra branch roles that humans cannot review predictably.

### `sprint-branch finish --task <task>`

`finish` answers "is this active task ready for human review?"

It marks an active `review` or `next` task as `finished-unreviewed`. The task
must already be active, and its task markdown must contain a complete Review
Report. Marking a task finished does not approve the task and does not move any
branch.

At most two tasks can be finished and unreviewed at once. This matches the two
active branch slots and keeps the human review queue bounded.

`finish` does not require a clean working tree because it only records review
readiness after the Review Report is complete.

Why it matters: it separates "the agent says this task is done" from "the human
has approved this task."

### `sprint-branch feedback`

`feedback` answers "how can the agent stop work-ahead and return to the review
branch for human-requested changes?"

It prepares the review branch for feedback work. If the current branch is
`next` and has local edits, the command preserves that interrupted work and
records it as active sprint work to resume later. It then moves the working
context back to `review`.

Dirty work outside the recorded `next` branch is blocked. The command is allowed
to preserve dirty `next` work because that is the interruption it is designed to
handle.

Task assignments do not move during feedback: the review task remains the
review task, and the next task remains the next task.

Why it matters: human feedback often arrives while an agent is working ahead.
This command protects the work-ahead changes while making the review branch
available for the feedback response.

### `sprint-branch resume`

`resume` answers "how does the agent return to interrupted or dependent
work-ahead after feedback?"

If a `next` task exists, `resume` returns to `next`, makes sure it is based on
the latest review content, and restores any recorded interrupted work for the
current next task. If no `next` task exists, it returns to `review`.

`resume` normally requires a clean working tree. The exception is the final
step of resolving a previously recorded resume conflict, where the working tree
may contain the resolved files needed to complete the retry.

When a resume-related conflict occurs, the sprint state remains at the
pre-resume boundary until the conflict is resolved and `resume` is retried.

Why it matters: work-ahead depends on review. After feedback changes review,
dependent next work must be brought forward before it can safely continue.

### `sprint-branch approve`

`approve` answers "how does human-approved review work become the new approved
boundary?"

It promotes the current review task into `approved`. The review task must exist,
be marked `finished-unreviewed`, have a complete Review Report, and be based on
the current approved branch. The working tree must be clean.

If no `next` task exists, approval advances `approved` to the review content,
records the task as approved, clears the review task, and leaves review ready
for the next start.

If a `next` task exists, approval first ensures the next work is based on the
review content, then advances `approved`, records the reviewed task as approved,
and rolls the next task forward so it becomes the new review task. The `next`
slot becomes empty.

When an approval conflict occurs, the reviewed task is not recorded as approved
until the conflict is resolved and `approve` is retried.

Why it matters: approval is the transition from "ready for human review" to
"accepted as the baseline for future sprint work."

### `sprint-branch rebase <target>`

`rebase` answers "how can the whole sprint branch stack move onto a newer base?"

It moves the recorded sprint branches onto a target ref while preserving the
rolling relationship between `approved`, `review`, and `next`. The target must
exist, must not be one of the sprint branches, and must share history with the
current approved branch. The working tree must be clean, and no interrupted
sprint stashes may be active.

The sprint's recorded base changes only after every relevant sprint branch has
successfully moved to the new target. If a conflict interrupts the transition,
the prior base remains recorded and `rebase` can be retried after conflict
resolution.

Why it matters: it keeps long-running sprint work current with the target branch
without collapsing the review boundary or losing the work-ahead relationship.

### `sprint-branch finalize [--override-base <ref>]`

`finalize` answers "is the fully approved sprint ready for the human's final
merge?"

It prepares the completed review branch for landing. Finalization requires no
active review task, no next task, and no finished-unreviewed tasks. Review and
approved must represent the same approved content, and `next` must not contain
different work. The working tree must be clean, and the base ref must resolve.

The command brings the completed review content onto the sprint's recorded base,
updates the approved boundary to match review, and leaves the review branch as
the branch humans land from. `--override-base` exists for recovery when the
recorded base is not the target humans intend to land onto.

If a finalize conflict occurs, sprint state remains at the pre-finalize boundary
until the conflict is resolved and `finalize` is retried.

Why it matters: approval finishes the task queue; finalization prepares the
whole approved sprint for a clean human landing.

