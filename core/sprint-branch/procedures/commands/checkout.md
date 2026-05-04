# `sprint-branch checkout [name]`

`checkout` answers "how can a human inspect the sprint review branch without
taking over the live branch?"

It checks out the selected sprint's review branch as a detached snapshot. The
review branch remains agent-owned; the human is looking at a commit snapshot,
not moving the branch forward by working on it directly.

If `name` is omitted, the sprint can be inferred from the current sprint branch
or `sprints/<name>` working directory. In an interactive terminal, the command
can prompt for an active sprint. Non-interactive callers must provide `name`
when no strong context exists.

`checkout` requires a clean working tree before switching snapshots. It supports
`--dry-run` to show which sprint and review branch would be checked out.

Why it matters: human review needs a stable, inspectable snapshot, while sprint
branches remain controlled by the workflow commands.
