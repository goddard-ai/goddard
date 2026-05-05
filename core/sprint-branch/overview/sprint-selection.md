# Sprint Selection

- **Purpose**
  - This page defines the shared sprint selection behavior used by commands
    that operate on one sprint.
  - Command pages refer here instead of repeating shared selection rules.
  - Command pages document only command-specific selectors or exceptions.

- **Standard selectors**
  - Explicit sprint selection takes precedence.
    - Most commands use `--sprint <name>`.
    - Human commands such as `checkout`, `land`, and `cleanup` also accept a
      positional `name` as their explicit sprint selector.
  - `-l` / `--last` selects the most recently acted-upon sprint.
  - Strong local context can select a sprint:
    - The current `sprint/<name>/<role>` branch.
    - A working directory under `sprints/<name>`.
  - Interactive terminals can prompt from active sprint state when no explicit
    selector or strong context identifies a sprint.

- **Non-interactive callers**
  - Non-interactive callers must provide one of:
    - An explicit sprint selector.
    - `-l` / `--last` after at least one sprint has recorded activity.
    - Strong local context.
  - State records alone are not enough for non-interactive inference because
    they may represent multiple active or parked sprints.
  - When a command cannot infer a sprint non-interactively, it reports available
    candidates where that is useful.

- **Parked sprints**
  - Parked sprints are omitted from prompted default selection.
  - Parked sprints remain selectable through:
    - An explicit sprint selector.
    - Strong local context.
    - `-l` / `--last` when the parked sprint is the latest acted-upon sprint.

- **Prompt ordering**
  - Prompted sprint choices are ordered by last activity.
  - Sprints without activity fall back to stable name order.

- **Activity tracking**
  - `sprint-branch` tracks when it last acted on each sprint so recent sprints
    can be selected and ordered predictably.
  - Inspection commands refresh activity after resolving a readable sprint.
  - Mutating commands refresh activity when they successfully complete.
  - Dry runs and failed mutations do not refresh activity.
  - `list` does not select a current sprint and does not refresh activity.
  - `cleanup` removes the selected sprint from future selection instead of
    refreshing activity.
  - Command pages describe workflow effects; they do not repeat this shared
    activity-tracking behavior in each `What it changes` section.
