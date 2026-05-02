# @goddard-ai/sprint-branch

`@goddard-ai/sprint-branch` provides the `sprint-branch` CLI for inspecting and safely transitioning sprint review branches.

The package stores sprint branch-management state in Git metadata at `.git/sprint-branch/<name>/state.json`, outside the working tree and outside review diffs. The `sprints/<name>/` folder remains for human-authored task files; successful sprint state writes add `sprints/` to `.git/info/exclude` so local sprint plans stay out of review diffs.

Commands infer a sprint only from explicit `--sprint`, the current `sprint/<name>/<role>` branch, or a working directory under `sprints/<name>`. When that strong context is missing, interactive terminals select from existing sprint state with autocomplete; non-interactive callers must pass the sprint name.

`sprint-branch checkout [name]` is the human review command: it checks out the sprint review branch as a detached snapshot so the live review branch remains agent-owned.

`sprint-branch park [--sprint <name>]` hides a sprint from default active-sprint selection without deleting branches or Git-private state. `sprint-branch unpark [--sprint <name>]` makes it active again. Use `sprint-branch list --all` to include parked sprints.

`sprint-branch land <target> [name]` and `sprint-branch cleanup <target> [name]` are human-only landing commands. Actual landing and cleanup require an interactive terminal confirmation; use `--dry-run` to inspect them non-interactively. Cleanup removes landed sprint branches, associated review worktrees, and the Git-private sprint state file.

`sprint-branch reset-state [--task <task>] [--force]` recreates only the Git-private state file after a sprint plan is reworked. It makes the first task, or the selected task, the next valid `start` target by recording earlier task files as approved; it does not move Git branches.

`sprint-branch rebase <target>` rebases the recorded approved, review, and existing next sprint branches onto a target Git ref. Git performs the rebases; sprint state records the new base ref only after every branch rebase succeeds.

`sprint-branch sync [--sprint <name>]` watches `review-sync` for the active sprint review branch in process, matching `review-sync watch <agent-branch>` after sprint resolution. The command resolves the active sprint the same way as read-only sprint commands: explicit `--sprint`, current sprint branch, `sprints/<name>` working directory, or interactive selection from active sprint state.

- `sprint-branch status`
- `sprint-branch diff`
- `sprint-branch sync`
- `sprint-branch doctor`
- `sprint-branch list [--all]`
- `sprint-branch checkout [name]`
- `sprint-branch land <target> [name]`
- `sprint-branch cleanup <target> [name]`
- `sprint-branch init`
- `sprint-branch reset-state`
- `sprint-branch park`
- `sprint-branch unpark`
- `sprint-branch start`
- `sprint-branch feedback`
- `sprint-branch resume`
- `sprint-branch approve`
- `sprint-branch rebase <target>`
- `sprint-branch finalize`

Agent workflow mutations support `--dry-run` and `--json`. Human landing commands support `--json` for `--dry-run` inspection, but actual landing and cleanup require an interactive terminal. The sprint state mutations use a Git-private lock file, write JSON state atomically, and only move recorded sprint branches.
