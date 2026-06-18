# Daemon-Managed Local Automation

The daemon is the local lifecycle authority for unattended automation.

## Participants
- Operator controlling local automation behavior
- Desktop workspace or another supervised local host
- SDK and approved operational CLI clients
- Automated agents running unattended work
- External reviewers whose feedback may trigger local execution

## Runtime Domains
The daemon may host multiple distinct automation domains, including:
- PR feedback handling
- Workforce orchestration for multi-agent delegation
- Pipeline runs for reusable linear handoff workflows

The daemon also owns shared launch behavior for fresh isolated session worktrees when supported session flows request worktree isolation.

## Boundaries
- The daemon is the lifecycle authority for supported runtimes.
- Client surfaces may control or observe runtimes, but they must not create parallel ownership of mutable runtime state.
- Distinct runtimes may share local infrastructure, but they must not share mutable execution state in ways that blur their responsibilities.
- Pipeline definitions are registered capabilities, while Pipeline run instances are daemon-owned runtime state.
- Daemon shutdown must stop hosted runtimes cleanly.
- The daemon remains a headless automation boundary rather than the primary human-facing workspace.
- The daemon does not replace the app as the primary human-facing surface.
- The daemon does not replace the SDK as the primary programmatic surface.
- This parent spec does not define command syntax, payload shapes, or storage mechanics.

## Rationale
Background automation moved toward daemon-owned runtime management because unattended local work benefits from one lifecycle authority, consistent recovery rules, and shared control surfaces.

## Runtime Specs

* `spec/daemon/pr-feedback.md`: PR feedback flow behavior.
* `spec/daemon/session-worktree-preparation.md`: Fresh isolated session worktree preparation and its trust boundaries.
* `spec/daemon/workforce.md`: Workforce orchestration for delegated multi-agent work.
* `spec/core/pipelines.md`: Pipeline definition, run, and step boundaries shared by daemon, SDK, and app surfaces.
