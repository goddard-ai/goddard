# `sprint-branch init [--base <ref>]`

- **Question it answers**
  - How does a sprint become branch-managed?

- **What it creates**
  - The initial sprint branch scaffold.
  - Initial sprint state.
  - `approved` and `review` at the base boundary.

- **Inputs**
  - `--base <ref>` sets the base ref.
  - The base defaults to `main`.

- **What it changes**
  - Creates sprint branches and records sprint state.
  - Leaves no task active.
  - Marks the sprint active for default selection.

- **Guardrails**
  - The sprint folder must already exist.
  - The base ref must resolve.
  - The expected sprint branches must not already exist.
  - The sprint state must not already exist.
  - A clean working tree is not required because the command creates the
    scaffold rather than moving existing work between sprint branches.

- **Why it exists**
  - It defines the review boundary before any task starts.
  - Starting from a known approved base makes later review diffs meaningful.
