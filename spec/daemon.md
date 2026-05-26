# Daemon-Managed Local Automation

The daemon is the local lifecycle authority for Goddard's unattended automation runtimes.

## Participants
- Operator controlling local automation behavior
- Desktop workspace or another supervised local host
- SDK and approved operational CLI clients
- Automated agents running unattended work
- External reviewers whose managed pull request feedback may trigger local execution

## Runtime Domains
The daemon may host multiple distinct automation domains, including:
- PR feedback handling for managed pull requests
- Repository-scoped workforce orchestration for multi-agent delegation

The daemon also owns shared launch behavior for fresh isolated session worktrees when supported session flows request worktree isolation.

## Boundaries
- The daemon is the lifecycle authority for supported daemon-managed runtimes.
- Client surfaces may control or observe daemon-managed runtimes, but they must not create parallel ownership of mutable runtime state.
- Distinct daemon-managed runtimes may share local infrastructure, but they must not share mutable execution state in ways that blur their responsibilities.
- Daemon shutdown must stop hosted runtimes cleanly.
- The daemon remains a headless automation boundary rather than the primary human-facing workspace.
- The daemon does not replace the desktop app as the primary human-facing surface.
- The daemon does not replace the SDK as the primary programmatic surface.
- This parent spec does not define command syntax, payload shapes, or storage mechanics.

## Rationale
Background automation moved toward daemon-owned runtime management because unattended local work benefits from one lifecycle authority, consistent recovery rules, and shared control surfaces.

## Runtime Specs

* `spec/daemon/pr-feedback.md`: PR feedback flow behavior for managed pull request feedback.
* `spec/daemon/session-worktree-preparation.md`: Fresh isolated session worktree preparation and its trust boundaries.
* `spec/daemon/workforce.md`: Daemon-owned repository workforce orchestration for delegated multi-agent work.
