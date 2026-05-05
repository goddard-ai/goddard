# `sprint-branch status`

- **Question it answers**
  - What is the current sprint branch state?

- **Sprint selection**
  - Uses [standard sprint selection](../sprint-selection.md).

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
  - Missing task files for active, unreviewed, or reviewable local tasks;
    missing branches; dirty worktrees; broken ancestry; and recorded recovery
    state are reported as diagnostics rather than hidden.
  - Already approved historical tasks are archival metadata:
    - Their local task files are useful context.
    - Missing local files do not create task-file diagnostics.
  - When the sprint is blocked:
    - The report identifies why.
    - The next safe command is included when one can be determined.

- **Why it exists**
  - Agents use it as the basic orientation command before choosing a workflow
    action.
  - Humans use it to understand whether a sprint is ready for review, blocked,
    or safe to continue.
