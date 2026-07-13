# Daemon-Managed Local Automation

The daemon is the local lifecycle authority for unattended automation.

## Participants
- Operator controlling local automation behavior
- Desktop workspace or another supervised local host
- Embedded desktop webview authorized by a trusted local host
- Hosted browser surface authorized through local pairing
- SDK and approved operational CLI clients
- Automated agents running unattended work
- External reviewers whose feedback may trigger local execution

## Runtime Domains
The daemon may host multiple distinct automation domains, including:
- PR feedback handling
- Workforce orchestration for multi-agent delegation

The daemon also owns shared launch behavior for fresh isolated session worktrees when supported session flows request worktree isolation.
It also owns durable repository task state used by clients and agents to coordinate planned work without turning tasks into an automation runtime.

## Boundaries
- The daemon is the lifecycle authority for supported runtimes.
- Client surfaces may control or observe runtimes, but they must not create parallel ownership of mutable runtime state.
- Distinct runtimes may share local infrastructure, but they must not share mutable execution state in ways that blur their responsibilities.
- Daemon shutdown must stop hosted runtimes cleanly.
- The daemon remains a headless automation boundary rather than the primary human-facing workspace.
- The daemon does not replace the app as the primary human-facing surface.
- The daemon does not replace the SDK as the primary programmatic surface.
- The daemon may expose loopback HTTP access to browser-origin clients only through explicit browser-access configuration, origin validation, host validation, Private Network Access-compatible preflight behavior, and bearer-token authorization.
- Hosted browser access must require local pairing before normal daemon IPC routes are usable. Pairing is disabled by default, uses explicit local confirmation, and issues durable tokens bound to the origin that completed pairing.
- Desktop webview access is distinct from hosted browser pairing. A trusted desktop host may request short-lived daemon-issued webview tokens for its embedded webview, but those tokens are valid only with the expected desktop webview origin and must not become durable browser trust.
- Trusted local daemon clients, including the desktop Bun host, SDK/node clients, and approved operational CLI clients, do not require hosted-browser pairing.
- Browser-origin access must fail closed for missing, malformed, unconfigured, or ambiguous origin and authorization state. The daemon must not authorize local browser control through cookies, wildcard origins, or CORS alone.
- This parent spec does not define command syntax, payload shapes, or storage mechanics.

## Rationale
Background automation moved toward daemon-owned runtime management because unattended local work benefits from one lifecycle authority, consistent recovery rules, and shared control surfaces.

## Runtime Specs

* `spec/daemon/pr-feedback.md`: PR feedback flow behavior.
* `spec/daemon/session-worktree-preparation.md`: Fresh isolated session worktree preparation and its trust boundaries.
* `spec/daemon/tasks.md`: Durable repository task planning and coordination.
* `spec/daemon/workforce.md`: Workforce orchestration for delegated multi-agent work.
