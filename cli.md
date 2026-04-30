# Operational CLI Surfaces

## Goal
Define the narrow CLI role that remains valid in Goddard: thin, daemon-backed operational control for local automation without reviving a parallel terminal-first product experience.

## Hypothesis
We believe that preserving a small operator CLI surface for initialization, inspection, and control of daemon-backed runtimes will improve operability while keeping primary human workflows centered on the desktop app and primary programmatic workflows centered on the SDK.

## Constraints
- Human-facing day-to-day workflows belong in the desktop app.
- Programmatic and embedded workflows belong in SDK consumers.
- Approved CLI surfaces may initialize repository-local automation intent and control daemon-backed local runtimes.
- CLI behavior must remain thin over SDK and daemon contracts rather than creating a parallel command-routing architecture.
- CLI support must stay narrow enough that Goddard does not drift back into a terminal-first primary UX.

## Non-Goals
- Reintroducing command-based authentication, pull request creation, spec editing, proposal review, or other broad product workflows as supported CLI flows
- Treating terminal-first interaction as the primary Goddard experience
- Reimplementing platform behavior outside the shared SDK and daemon authority model

## Decision Memory
The broad interactive CLI was removed when product focus narrowed to an SDK-first platform plus a desktop workspace. A narrow daemon-backed operational CLI remains valuable for automation bring-up, inspection, and control.

## Encapsulated Sub-Specs

* `spec/cli/interactive.md`: Tombstone for removed interactive CLI workflows.
* `spec/cli/loop.md`: Tombstone for removed autonomous loop CLI workflows.
* `spec/cli/operational.md`: Supported narrow CLI behavior for daemon-backed operational control.
