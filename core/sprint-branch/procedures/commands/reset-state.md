# `sprint-branch reset-state [--task <task>] [--base <ref>] [--force]`

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
