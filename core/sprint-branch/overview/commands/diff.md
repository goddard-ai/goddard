# `sprint-branch diff [-l|--last]`

- **Question it answers**
  - What changed between the approved boundary and the review boundary?

- **Sprint selection**
  - The sprint comes from normal sprint resolution, `-l` / `--last`, or an
    explicit `--sprint`.
  - Non-interactive callers need a sprint argument, `-l` after recorded
    activity, or strong local context.
  - `-l` / `--last` selects the most recently acted-upon sprint.

- **What it reports**
  - The review branch delta against approved work.
    - The comparison is the sprint review boundary, not every commit reachable
      from either branch.
  - Optional formats:
    - `--name-only` narrows output to changed paths.
    - `--stat` shows changed-file and line-count summaries.
  - JSON output includes the underlying diff command and its output for agents
    that need to relay or inspect it.

- **What it changes**
  - It does not change branches, task state, review state, or working tree
    files.
  - It records the selected sprint's private `lastActedAt` timestamp.

- **Guardrails**
  - The sprint status must be readable enough to identify the branch roles.
  - The approved branch must exist.
  - The review branch must exist.
  - The review branch must be based on approved work.
  - If the review branch no longer descends from approved, the command refuses
    instead of showing a misleading comparison.

- **Why it exists**
  - It is the sprint-level review comparison.
  - It keeps review focused on unapproved sprint work instead of the full
    repository history.
