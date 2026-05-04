# `sprint-branch diff`

`diff` answers "what changed between the approved boundary and the review
boundary?"

It shows the review branch delta against approved work. `--name-only` narrows
the output to changed paths. `--stat` shows a summary of changed files and line
counts.

`diff` requires the approved and review branches to exist and the review branch
to be based on approved work. It does not change anything.

Why it matters: this is the sprint-level review comparison. It keeps review
focused on unapproved work instead of the full repository history.
