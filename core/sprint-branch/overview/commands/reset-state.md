# `sprint-branch reset-state [--task <task>] [--base <ref>] [--force]`

- **Question it answers**
  - How can private sprint state be recreated after a sprint plan is reworked
    or state is missing?

- **What it does**
  - Rewrites only private sprint state.
  - Does not move sprint branches.
  - Makes the selected task the next valid `start` target.
  - If no task is selected:
    - The first task in sprint task-file order becomes next.
  - Tasks before the selected task are recorded as already approved.

- **Inputs**
  - `--task <task>` selects the next start target.
  - `--base <ref>` records a base when existing state cannot supply one.
  - `--force` allows certain active-work and branch-drift blockers to become
    warnings.

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
  - `--force` should only be used after preserving or intentionally discarding
    the work described by warnings.

- **Why it exists**
  - It gives agents a recovery path when the human-authored sprint plan changes.
  - It avoids pretending to safely move branch content that it is not moving.
