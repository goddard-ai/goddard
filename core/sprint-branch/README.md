# @goddard-ai/sprint-branch

`@goddard-ai/sprint-branch` provides the `sprint-branch` CLI for inspecting and safely transitioning sprint review branches.

The package owns sprint branch-management state under `sprints/<name>/.sprint-branch-state.json` and treats `sprints/<name>/000-index.md` as a human-readable mirror. It appends concise transition notes to `sprints/<name>/001-handoff.md`.

`sprint-branch checkout [name]` is the human review command: it checks out the sprint review branch as a detached snapshot so the live review branch remains agent-owned.

`sprint-branch land <target> [name]` and `sprint-branch cleanup <target> [name]` are human-only landing commands. Actual landing and cleanup require an interactive terminal confirmation; use `--dry-run` to inspect them non-interactively.

- `sprint-branch status`
- `sprint-branch diff`
- `sprint-branch doctor`
- `sprint-branch checkout [name]`
- `sprint-branch land <target> [name]`
- `sprint-branch cleanup <target> [name]`
- `sprint-branch init`
- `sprint-branch start`
- `sprint-branch feedback`
- `sprint-branch resume`
- `sprint-branch approve`
- `sprint-branch finalize`

Agent workflow mutations support `--dry-run` and `--json`. Human landing commands support `--json` for `--dry-run` inspection, but actual landing and cleanup require an interactive terminal. The sprint state mutations use a Git-private lock file, write JSON state atomically, and only move recorded sprint branches.
