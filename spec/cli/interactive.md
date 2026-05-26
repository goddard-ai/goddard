# Interactive CLI Removal

## Status
Interactive `goddard` commands are removed and are not supported product behavior.

## Replacement Surfaces
- Authentication, pull request initiation, specification work, and proposal workflows belong in the desktop app.
- Programmatic integrations must use the SDK directly.

## Boundaries
- Goddard does not preserve command parity with the removed interactive CLI.
- This spec does not document shell flags, terminal prompts, or command exit behavior for removed workflows.

## Rationale
Interactive workflows were consolidated into the desktop app so Goddard has one primary human-facing surface instead of parallel terminal and desktop experiences.
