# `sprint-branch doctor`

- **Question it answers**
  - What is unsafe or inconsistent, and what should happen next?

- **Sprint selection**
  - The sprint comes from normal sprint resolution or an explicit `--sprint`.
  - Non-interactive callers need a sprint argument or strong local context.

- **What it reports**
  - The broad inspection surface from `status`.
  - Deeper consistency checks, including:
    - Missing branch roles.
    - Unexpected branch ancestry.
    - Unrecorded branch work.
    - Dirty working trees that make branch movement unsafe.
    - Task ordering problems.
    - Duplicate task assignments.
    - Tasks assigned to more than one role.
    - Finished-unreviewed tasks that are missing, duplicated, already approved,
      or otherwise outside the active review queue.
    - Incomplete Review Reports for finished tasks.
    - Recorded conflicts.
    - Active Git operations.
    - Stale recovery state.
    - Active sprint stashes.
    - Stash records that no longer match the recorded branch, task, or stash
      entry.
    - Task files with ambiguous ordering or unusual names.
    - Extra sprint namespace branches.
    - Current-branch situations that make deletion or mutation unsafe.

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
  - It can also direct the user back to `doctor` when manual cleanup or conflict
    resolution is still required before a workflow command is safe.

- **Why it exists**
  - It is the first command to run when the workflow no longer feels linear.
  - It keeps agents from guessing which branch or task state is safe to mutate.
