# `sprint-branch park`

- **Question it answers**
  - How can this sprint stay recorded but stop appearing in default active
    selection?

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

- **Guardrails**
  - A clean working tree is not required.

- **Why it exists**
  - Parking keeps inactive or paused sprint state available without making
    agents choose among stale active candidates.
