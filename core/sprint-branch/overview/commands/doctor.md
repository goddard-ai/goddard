# `sprint-branch doctor`

- **Question it answers**
  - What is unsafe or inconsistent, and what should happen next?

- **What it reports**
  - The broad inspection surface from `status`.
  - Deeper consistency checks, including:
    - Missing branch roles.
    - Unexpected branch ancestry.
    - Unrecorded branch work.
    - Task ordering problems.
    - Duplicate task assignments.
    - Incomplete Review Reports for finished tasks.
    - Recorded conflicts.
    - Active Git operations.
    - Stale recovery state.
    - Active sprint stashes.
    - Extra sprint namespace branches.

- **What it changes**
  - Nothing.
  - It does not repair state by itself.

- **Recovery guidance**
  - It explains the problem.
  - When possible, it names the next safe command.
  - Typical next commands include retrying:
    - `resume`.
    - `approve`.
    - `rebase`.
    - `finalize`.

- **Why it exists**
  - It is the first command to run when the workflow no longer feels linear.
  - It keeps agents from guessing which branch or task state is safe to mutate.
