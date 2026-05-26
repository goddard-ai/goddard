# Loop CLI Removal

## Status
`goddard loop` is removed and is not a supported command surface.

## Remaining Intent
Autonomous loop execution remains a platform capability, but it must be hosted by the desktop app or another SDK-based supervisor rather than a built-in CLI command.

## Boundaries
- This spec does not document config loading, stdout behavior, or exit codes for a removed command.
- Goddard does not reintroduce a dedicated terminal entry point for autonomous loop control.

## Rationale
The loop command was removed to keep autonomous control SDK-first and to avoid maintaining a parallel operator experience outside the desktop workspace.
