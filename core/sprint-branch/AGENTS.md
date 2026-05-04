# sprint-branch Agent Notes

- Before answering conceptual questions about `sprint-branch` or changing
  overview docs, read `overview/README.md`.
- Keep overview documentation durable, conceptual, and scan-first:
  - Use nested bullet outlines heavily.
  - Prefer bold root bullets such as `**Purpose**` over additional hash
    headings inside overview pages.
  - Describe what is supported, what changes, and why guardrails exist.
  - Do not force a fixed page template; shape each overview page around the
    command or procedure's actual risk and complexity.
  - Document handled edge cases when they are externally observable, affect
    workflow decisions, define a safety boundary, or explain recovery/retry
    behavior.
  - Usually omit edge cases that only expose internal helper behavior, exact
    diagnostic wording, parser details, lock mechanics, atomic write strategy,
    or implementation order.
  - Avoid implementation walkthroughs.
- When adding, removing, renaming, or changing the meaning of a command, update
  the relevant file in `overview/commands/` and the index in
  `overview/README.md`.
- Prefer contract tests that assert externally observable behavior documented in
  `overview/`. Add regression tests when protecting a known bug or subtle
  safety invariant, and include a short comment naming the risk. Add internal
  tests cautiously, only for complex pure logic or safety-critical invariants
  that are hard to cover through public commands.
- Avoid tests that only prove one first-party wrapper calls another, freeze
  incidental wording or helper structure, or assert behavior that neither
  appears in `overview/` nor protects a named regression risk.
