# `sprint-branch status`

- **Question it answers**
  - What is the current sprint branch state?

- **What it reports**
  - Resolved sprint and current branch.
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

- **Why it exists**
  - Agents use it as the basic orientation command before choosing a workflow
    action.
  - Humans use it to understand whether a sprint is ready for review, blocked,
    or safe to continue.
