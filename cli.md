# CLI Surface Removal

This node records that Goddard no longer ships or supports a CLI / `cmd` package as an active product surface.

## Goal
Make the removal explicit so product intent stays centered on two supported surfaces: the SDK for programmatic integration and the Tauri desktop app for human-facing workflows.

## Constraints
- Human-facing workflows belong in the desktop app.
- Programmatic and embedded workflows belong in SDK consumers.
- No active product behavior should depend on terminal commands or a parallel command-routing architecture.

## Non-Goals
- Reintroducing command-based authentication, PR creation, spec editing, proposal review, or loop control as supported product flows.
- Treating terminal-first interaction as the primary UX for Goddard.

## Decision Memory
The CLI was removed when product focus narrowed to an SDK-first platform plus a Tauri desktop workspace. Future local automation hosts should build on the SDK rather than resurrect a separate command surface.

## Encapsulated Sub-Specs

* `spec/cli/interactive.md`: Tombstone for removed interactive CLI workflows.
* `spec/cli/loop.md`: Tombstone for removed autonomous loop CLI workflows.
