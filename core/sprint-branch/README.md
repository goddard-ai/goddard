# @goddard-ai/sprint-branch

`@goddard-ai/sprint-branch` provides the `sprint-branch` CLI for inspecting and safely transitioning sprint review branches.

The package owns sprint branch-management state under `sprints/<name>/.sprint-branch-state.json` and treats `sprints/<name>/000-index.md` as a human-readable mirror. It appends concise transition notes to `sprints/<name>/001-handoff.md`.

`sprint-branch checkout [name]` is the human review command: it checks out the sprint review branch as a detached snapshot so the live review branch remains agent-owned.

- `sprint-branch status`
- `sprint-branch diff`
- `sprint-branch doctor`
- `sprint-branch checkout [name]`
- `sprint-branch init`
- `sprint-branch start`
- `sprint-branch feedback`
- `sprint-branch resume`
- `sprint-branch approve`
- `sprint-branch finalize`

Every mutating command supports `--dry-run` and `--json`. The mutating commands use a Git-private lock file, write JSON state atomically, and only move recorded sprint branches.
