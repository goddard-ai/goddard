# `sprint-branch reset-state [--task <task>] [--base <ref>] [--force]`

- **Question it answers**
  - How can private sprint state be recreated after a sprint plan is reworked
    or state is missing?

- **Inputs and selection**
  - The sprint comes from normal sprint resolution or an explicit `--sprint`.
  - `--task <task>` selects the next task that `start` should accept.
    - The task may be named by its task-file stem or matching task filename.
    - If no task is selected, the first task in sprint task-file order becomes
      next.
  - `--base <ref>` records a base when existing state cannot supply one.
    - If neither `--base` nor existing readable state supplies a base, the base
      defaults to `main`.
  - `--force` allows certain active-work and branch-drift blockers to become
    warnings.

- **What it does**
  - Rewrites only private sprint state.
  - Does not move sprint branches.
  - Makes the selected task the next valid `start` target.
  - Tasks before the selected task are recorded as already approved.
  - Preserves existing sprint visibility when readable state exists.
  - Clears active task assignments, finished-unreviewed records, interrupted
    sprint work, and recorded conflicts.
  - If existing private state is missing or unreadable:
    - Creates a replacement state record.
    - Reports the state problem as a diagnostic.

- **Guardrails**
  - Requires:
    - A clean working tree.
    - An existing sprint folder.
    - At least one task file.
    - A resolvable base ref.
    - Existing approved and review branches.
  - Checks whether current branch contents appear to contain:
    - Active sprint work.
    - Unrecorded sprint work.
    - Interrupted sprint work.
    - Recorded conflict recovery.
  - `--force` should only be used after preserving or intentionally discarding
    the work described by warnings.
  - `--force` does not move branches.
    - The command may still leave branch content that `doctor` reports later.

- **Dry run**
  - Reports the state record that would be written.
  - Does not update private sprint state.

- **Why it exists**
  - It gives agents a recovery path when the human-authored sprint plan changes.
  - It avoids pretending to safely move branch content that it is not moving.
