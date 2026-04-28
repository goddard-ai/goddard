# @goddard-ai/sprint-branch

`@goddard-ai/sprint-branch` provides the `sprint-branch` CLI for inspecting and, in later phases, safely transitioning sprint review branches.

The package owns sprint branch-management state under `sprints/<name>/.sprint-branch-state.json` and treats `sprints/<name>/000-index.md` as a human-readable mirror. Phase 1 implements the read-only commands:

- `sprint-branch status`
- `sprint-branch diff`
- `sprint-branch doctor`

The mutating workflow command names are reserved in the CLI and currently fail with an explicit not-implemented message until their safety checks are implemented.
