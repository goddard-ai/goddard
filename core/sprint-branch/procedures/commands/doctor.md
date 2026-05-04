# `sprint-branch doctor`

`doctor` answers "what is unsafe or inconsistent, and what should happen next?"

It starts from the same broad inspection surface as `status`, then applies
deeper consistency checks. It looks for states such as missing branch roles,
unexpected branch ancestry, unrecorded branch work, task ordering problems,
duplicate task assignments, incomplete Review Reports for finished tasks,
recorded conflicts, active Git operations, stale recovery state, active sprint
stashes, and extra sprint namespace branches.

`doctor` does not repair state by itself. Its output explains the problem and,
when possible, names the next safe command, such as retrying `resume`,
`approve`, `rebase`, or `finalize` after a conflict has been resolved.

Why it matters: it is the first command to run when the workflow no longer feels
linear. It keeps agents from guessing which branch or task state is safe to
mutate.
