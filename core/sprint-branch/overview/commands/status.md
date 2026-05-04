# `sprint-branch status`

- **Question it answers**
  - What is the current sprint branch state?

- **Sprint selection**
  - The sprint comes from normal sprint resolution or an explicit `--sprint`.
  - Non-interactive callers need a sprint argument or strong local context.
  - State records alone are not enough for non-interactive inference because
    they may represent multiple active or parked sprints.

- **What it reports**
  - Resolved sprint and current branch.
  - How the sprint was resolved.
  - The private state location in human-readable terms.
  - Sprint visibility.
  - Branch roles, existence, and ancestry.
  - Working tree cleanliness.
  - Task assignments and task queue.
  - Finished-but-unreviewed tasks.
  - Diagnostics.
  - The next safe command when one is known.

- **What it changes**
  - Nothing.
  - It does not change branches, task state, review state, or working tree
    files.

- **Guardrails**
  - It can fail when:
    - The sprint cannot be inferred.
    - The recorded state is too invalid to read.
  - Missing task files, missing branches, dirty worktrees, broken ancestry, and
    recorded recovery state are reported as diagnostics rather than hidden.
  - When the sprint is blocked:
    - The report identifies why.
    - The next safe command is included when one can be determined.

- **Why it exists**
  - Agents use it as the basic orientation command before choosing a workflow
    action.
  - Humans use it to understand whether a sprint is ready for review, blocked,
    or safe to continue.
