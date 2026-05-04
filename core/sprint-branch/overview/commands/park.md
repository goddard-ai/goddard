# `sprint-branch park`

- **Question it answers**
  - How can this sprint stay recorded but stop appearing in default active
    selection?

- **Inputs and selection**
  - The sprint comes from normal sprint resolution or an explicit `--sprint`.
  - Explicit or strong-context selection can still target a parked sprint after
    parking.

- **What it does**
  - Marks the sprint as parked.
  - Keeps sprint branches and state intact.
  - Allows explicit sprint selection to keep working.
  - Hides the sprint from:
    - Default active selection.
    - `list` output unless `--all` is used.

- **What it changes**
  - Sprint visibility state only.
  - No branches move.
  - If the sprint is already parked, reports that state without changing branch
    or task content.

- **Guardrails**
  - A clean working tree is not required.
  - Is blocked while another sprint transition is waiting for conflict
    recovery.

- **Dry run**
  - Reports that the sprint would become parked.
  - Does not update sprint visibility state.

- **Why it exists**
  - Parking keeps inactive or paused sprint state available without making
    agents choose among stale active candidates.
