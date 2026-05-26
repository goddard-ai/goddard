# Desktop App Intent Index

The desktop app is the primary human-facing interface to the Goddard runtime. It is not a forked product surface with separate logic. The app should reflect the same daemon-backed control surfaces and backend-owned real-time activity used by other platform consumers.

The desktop app provides a unified workspace where developers can run sessions, review outputs, steer work, and move across repository, GitHub, spec, task, and roadmap context.

## Users
- Developer/operator managing one or more repositories
- Reviewer giving feedback on AI output
- Maintainer monitoring throughput and blockers

## Boundaries
- The app remains lightweight.
- High-churn views handle streaming updates gracefully.
- The app must not reintroduce a broad parallel CLI or other terminal-first primary workflow surface.
- The app must not implement a full in-app code editor.

## App Specs

* `spec/app/shell.md`: Desktop workspace shell, navigation model, and tab behavior.
* `spec/app/workflows.md`: Human-facing domain workflows and their visible lifecycle states.
* `spec/app/data-boundaries.md`: Shared app data requirements, lazy authentication, streaming, and trusted host constraints.
