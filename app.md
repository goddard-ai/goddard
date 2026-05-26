# Desktop App Intent Index

## Goal
Provide a unified desktop workspace for Goddard operations so developers can run sessions, review outputs, and steer work from one visual surface instead of fragmented repository, GitHub, and chat tooling.

## Hypothesis
We believe that consolidating sessions, pull requests, specs, tasks, and roadmap context into one desktop app will reduce context switching and speed up AI-assisted delivery.

## Big Picture
The desktop app is the primary human-facing interface to the Goddard runtime. It is not a forked product surface with separate logic. The app should reflect the same daemon-backed control surfaces and backend-owned real-time activity used by other platform consumers.

## Primary Actors
- Developer/operator managing one or more repositories
- Reviewer giving feedback on AI output
- Maintainer monitoring throughput and blockers

## Cross-Cutting Constraints
- Must remain lightweight.
- Must handle streaming updates gracefully for high-churn views.

## Non-Goals
- Reintroducing a broad parallel CLI or other terminal-first primary workflow surface.
- Implementing a full in-app code editor.

## Encapsulated Sub-Specs

* `spec/app/shell.md`: Desktop workspace shell, navigation model, and tab behavior.
* `spec/app/workflows.md`: Human-facing domain workflows and their visible lifecycle states.
* `spec/app/data-boundaries.md`: Shared app data requirements, lazy authentication, streaming, and trusted host constraints.
