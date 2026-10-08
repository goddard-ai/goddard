# Employee worktree submission

Employees assigned to daemon-managed Git worktrees land their own completed
unit with:

```sh
goddard-agent merge submit
```

Submission is opt-in per project — the flag lives on the registered project
and survives daemon restarts. Until the boss enables it with
`goddard-agent boss '{"type":"setProjectSubmissions","project":"<name>","enabled":true}'`,
`merge submit` fails with "submissions not enabled for this project".

The QA branch defaults to the daemon-global setting (normally `dev`), and the
boss can retarget a single project with
`goddard-agent boss '{"type":"setProjectQaBranch","project":"<name>","branch":"<branch>"}'`
— the override covers both `merge submit` landings and the review train; omit
`branch` to clear it. The named branch must be checked out somewhere in the
project's repository for submissions to land on it.

The daemon queues submissions in arrival order, checks that the employee's
recorded worktree is clean, rebases it onto the configured QA branch (normally
`dev`), skips patch-equivalent commits, and squashes the submitted changes
into one reviewable commit per employee unit. It runs `git diff --check` and
any repository-local `agent-merge.verify` commands, then fast-forwards the
already checked-out QA worktree. The unit commit carries the original messages
and every `Test-Plan:` trailer. A successful command prints the landed SHA.

Do not edit the worktree while submission is running. If the rebase conflicts,
the command leaves the rebase in progress and leaves the QA branch unchanged;
resolve the conflict in the employee worktree, continue the rebase, rerun the
required verification, commit any repair, and submit again. Verification or
checkout failures also leave the QA branch unchanged. Report conflicts and
failures to the boss with `goddard-agent boss` using `reportBlocker`, or include
the outcome in the task finish. Never claim a unit landed until the command
returns its SHA.

Bosses should include “agent-merge per rules” in assignments that use a
daemon-managed worktree. The assigned employee commits, submits, and reports
the SHA; a separate Worktree Integrator employee is not needed.

For an update while working, use the employee-facing command:

```sh
goddard-agent steer-supervisor --text 'The focused checks passed; submission is next.'
```

No task ID is needed: the daemon resolves the employee's supervisor, falling
back to the boss when that supervisor has expired. The message steers a live
turn or immediately starts a new one, even while the sender is still working.
It never waits in a prompt queue. A supervisor that cannot accept the message
returns an error; retry when it is available. Legacy employee `prompt` calls
to the supervisor use the same delivery, even with `--delivery queue`; any
other target is rejected. Use `boss report-blocker` only when supervisor or
human action is required to proceed.
