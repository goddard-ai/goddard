# `sprint-branch list [--all]`

`list` answers "which sprint branch states are known?"

By default, it lists active sprints. `--all` includes parked sprints. Each entry
shows the sprint name, visibility, and review branch.

`list` does not infer a current sprint and does not change anything. Unreadable
sprint state appears as diagnostics rather than stopping the entire listing.

Why it matters: it is the low-risk discovery command for choosing a sprint when
the current directory or branch does not identify one.
