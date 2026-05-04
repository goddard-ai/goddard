# `sprint-branch init [--base <ref>]`

- **Question it answers**
  - How does a sprint become branch-managed?

- **Inputs and selection**
  - The sprint comes from normal sprint resolution or an explicit `--sprint`.
  - `--base <ref>` selects the initial approved boundary.
    - The base defaults to `main`.

- **What it creates**
  - The initial sprint branch scaffold.
  - Initial sprint state.
  - `approved` and `review` at the base boundary.
  - An active sprint record with no task assigned yet.

- **What it changes**
  - Creates sprint branches and records sprint state.
  - Leaves no task active.
  - Marks the sprint active for default selection.

- **Guardrails**
  - The sprint folder must already exist.
  - The base ref must resolve.
  - The expected sprint branches must not already exist.
  - A bare branch at the sprint namespace must not already exist.
    - This avoids ambiguity between the namespace used for sprint branches and a
      real branch with the same prefix.
  - The sprint state must not already exist.
  - A clean working tree is not required because the command creates the
    scaffold rather than moving existing work between sprint branches.

- **Dry run**
  - Reports the scaffold that would be created.
  - Does not create branches.
  - Does not write sprint state.

- **Why it exists**
  - It defines the review boundary before any task starts.
  - Starting from a known approved base makes later review diffs meaningful.
