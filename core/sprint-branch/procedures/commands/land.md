# `sprint-branch land <target> [name]`

`land` answers "how does finalized sprint work enter the target branch?"

It fast-forwards a human-selected target branch, such as `main`, to the finalized
sprint review content. It is a human landing command and requires interactive
confirmation for real execution. `--dry-run --json` is supported for
non-interactive inspection.

The sprint must be finalized before it can land:

- no task may still be assigned to `review` or `next`;
- no task may remain `finished-unreviewed`;
- no sprint conflict may be recorded;
- no interrupted sprint stash may remain active;
- `review` and `approved` must represent the same finalized content;
- `next`, if present, must not contain different work;
- the target branch must exist and must not itself be a sprint branch;
- the target must be able to fast-forward to the finalized review content.

`land` does not delete sprint branches or sprint state. It only moves the target
branch when the finalized review content is ready to become target-branch
content.

Why it matters: it separates final human merge authority from agent workflow
approval. Agents can prepare a sprint, but landing remains a deliberate human
operation.
