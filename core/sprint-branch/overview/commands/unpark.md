# `sprint-branch unpark`

- **Question it answers**
  - How can a parked sprint become active again?

- **What it does**
  - Marks the sprint as active for default selection.

- **What it changes**
  - Sprint visibility state only.
  - It does not:
    - Move branches.
    - Change task assignments.
    - Require a clean working tree.

- **Why it exists**
  - It restores a paused sprint to the normal command-selection flow without
    reconstructing state.
