# `sprint-branch rebase <target>`

`rebase` answers "how can the whole sprint branch stack move onto a newer base?"

It moves the recorded sprint branches onto a target ref while preserving the
rolling relationship between `approved`, `review`, and `next`. The target must
exist, must not be one of the sprint branches, and must share history with the
current approved branch. The working tree must be clean, and no interrupted
sprint stashes may be active.

The sprint's recorded base changes only after every relevant sprint branch has
successfully moved to the new target. If a conflict interrupts the transition,
the prior base remains recorded and `rebase` can be retried after conflict
resolution.

Why it matters: it keeps long-running sprint work current with the target branch
without collapsing the review boundary or losing the work-ahead relationship.
