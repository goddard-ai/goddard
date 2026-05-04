# `sprint-branch init [--base <ref>]`

`init` answers "how does a sprint become branch-managed?"

It creates the initial sprint branch scaffold and records initial sprint state.
The base ref defaults to `main` unless `--base` is provided. The sprint folder
must already exist, the base ref must resolve, and the expected sprint branches
and state must not already exist.

After initialization, `approved` and `review` start at the base boundary, no task
is active, and the sprint is active for default selection.

`init` does not require a clean working tree because it creates the scaffold
rather than moving existing work between sprint branches.

Why it matters: it defines the review boundary before any task starts. Starting
from a known approved base is what makes later review diffs meaningful.
