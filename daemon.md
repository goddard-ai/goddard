# Daemon-Managed Local Automation

## Goal
Provide a single local authority for Goddard's unattended automation runtimes so background work can be started, observed, recovered, and shut down consistently.

## Hypothesis
We believe that centralizing runtime lifecycle inside a daemon will improve operability, recovery, and client consistency compared with splitting runtime ownership across multiple local tools.

## Primary Actors
- Operator controlling local automation behavior
- Desktop workspace or another supervised local host
- SDK and approved operational CLI clients
- Automated agents running unattended work
- External reviewers whose repository feedback may trigger local execution

## Runtime Domains
The daemon may host multiple distinct automation domains, including:
- PR-feedback one-shot handling for managed pull requests
- Repository-scoped workforce orchestration for multi-agent delegation

## Hard Constraints
- The daemon is the lifecycle authority for supported daemon-managed runtimes.
- Client surfaces may control or observe daemon-managed runtimes, but they must not create parallel ownership of mutable runtime state.
- Distinct daemon-managed runtimes may share local infrastructure, but they must not share mutable execution state in ways that blur their responsibilities.
- Daemon shutdown must stop hosted runtimes cleanly.
- The daemon remains a headless automation boundary rather than the primary human-facing workspace.

## Non-Goals
- Replacing the desktop app as the primary human-facing surface
- Replacing the SDK as the primary programmatic surface
- Defining command syntax, payload shapes, or storage mechanics in this parent spec

## Decision Memory
Background automation moved toward daemon-owned runtime management because unattended local work benefits from one lifecycle authority, consistent recovery rules, and shared control surfaces.

## Encapsulated Sub-Specs

* `spec/daemon/pr-feedback.md`: One-shot daemon behavior for managed pull request feedback.
* `spec/daemon/workforce.md`: Daemon-owned repository workforce orchestration for delegated multi-agent work.
