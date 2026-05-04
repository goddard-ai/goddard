# `sprint-branch status`

`status` answers "what is the current sprint branch state?"

It reports the resolved sprint, current branch, sprint visibility, branch roles,
branch existence, branch ancestry, working tree cleanliness, task assignments,
task queue, finished-but-unreviewed tasks, diagnostics, and the next safe
command when one is known.

`status` does not change branches, task state, review state, or working tree
files. It can still fail when the sprint cannot be inferred or the recorded
state is too invalid to read.

Why it matters: agents use `status` as the basic orientation command before
choosing a workflow action. Humans can use it to understand whether a sprint is
ready for review, blocked, or safe to continue.
