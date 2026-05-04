# `sprint-branch unpark [-l|--last]`

- **Question it answers**
  - How can a parked sprint become active again?

- **Inputs and selection**
  - The sprint comes from normal sprint resolution, `-l` / `--last`, or an
    explicit `--sprint`.
  - `-l` / `--last` selects the most recently acted-upon sprint.
  - Explicit or strong-context selection can target the parked sprint that is
    being restored.

- **What it does**
  - Marks the sprint as active for default selection.
  - If the sprint is already active, reports that state without changing branch
    or task content.

- **What it changes**
  - Sprint visibility state only.
  - The sprint's private `lastActedAt` timestamp.
  - It does not:
    - Move branches.
    - Change task assignments.
    - Require a clean working tree.

- **Guardrails**
  - Is blocked while another sprint transition is waiting for conflict
    recovery.

- **Dry run**
  - Reports that the sprint would become active.
  - Does not update sprint visibility state.

- **Why it exists**
  - It restores a paused sprint to the normal command-selection flow without
    reconstructing state.
