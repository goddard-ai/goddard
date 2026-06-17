# Git

Read this ruleset before staging, committing, reviewing diffs, splitting work, or finishing any file-changing task.

- Use Conventional Commits: `<type>(optional-scope): <description>`.
- Commit requested changes without waiting for a separate prompt.
- Split docs-only or policy-only changes from behavior or test changes unless inseparable.
- Keep commits atomic and single-purpose: commit independent fixes separately, and group changes only when they are required for the same fix.
- Before committing, review `git diff` and consider whether any added or changed logic would benefit from code comments.
- Include a commit body when the reason, tradeoffs, risks, or migration notes are not obvious from the subject.
- Do not spend commit-message space summarizing file categories the diff already reveals.
- In non-interactive terminals, set `GIT_EDITOR=true` for commands that would otherwise open an editor.
- Stage only intended changes. When staging partial changes, prefer `git-stage-lines FILE RANGES`; use `git add` only when the whole file should be staged.
