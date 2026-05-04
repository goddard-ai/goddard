# `sprint-branch park`

`park` answers "how can this sprint stay recorded but stop appearing in default
active selection?"

It marks the sprint as parked. Parked sprints still retain their branches and
state, and can still be selected explicitly. They are hidden from default active
selection and from `list` unless `--all` is used.

`park` does not move branches or require a clean working tree.

Why it matters: parking keeps inactive or paused sprint state available without
making agents choose among stale active candidates.
