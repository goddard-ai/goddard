# `sprint-branch diff`

- **Question it answers**
  - What changed between the approved boundary and the review boundary?

- **What it reports**
  - The review branch delta against approved work.
  - Optional formats:
    - `--name-only` narrows output to changed paths.
    - `--stat` shows changed-file and line-count summaries.

- **What it changes**
  - Nothing.

- **Guardrails**
  - The approved branch must exist.
  - The review branch must exist.
  - The review branch must be based on approved work.

- **Why it exists**
  - It is the sprint-level review comparison.
  - It keeps review focused on unapproved sprint work instead of the full
    repository history.
