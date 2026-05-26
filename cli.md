# Operational CLI Surfaces

The CLI is a thin operational control surface for local automation.

## Boundaries
- Day-to-day workflows belong in the app.
- Programmatic and embedded workflows belong in SDK consumers.
- Approved CLI surfaces may initialize repository-local automation intent and control daemon runtimes.
- CLI behavior must remain thin over SDK and daemon contracts rather than creating a parallel command-routing architecture.
- CLI support must stay narrow enough that the product does not drift back into a terminal-first primary UX.
- The CLI must not reintroduce command-based authentication, pull request creation, spec editing, proposal review, or other broad product workflows.
- Terminal-first interaction is not the primary experience.
- CLI behavior must not reimplement platform behavior outside the shared SDK and daemon authority model.

## Rationale
The broad interactive CLI was removed when product focus narrowed to an SDK-first platform plus the app. A narrow operational CLI remains valuable for automation bring-up, inspection, and control.

## CLI Specs

* `spec/cli/interactive.md`: Tombstone for removed interactive CLI workflows.
* `spec/cli/loop.md`: Tombstone for removed autonomous loop CLI workflows.
* `spec/cli/operational.md`: Supported narrow CLI behavior for daemon operational control.
