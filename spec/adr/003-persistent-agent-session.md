---
id: adr-003-persistent-agent-session
status: ACTIVE
links:
  - type: Relates-To
    target: spec/runtime-loop.md
  - type: Relates-To
    target: spec/configuration.md
---

# ADR-003: Persistent Agent Session Across Cycles

## Status
ACTIVE

## Context

The autonomous loop drives multiple `pi-coding-agent` cycles against a codebase. The question was whether to create a fresh agent session for each cycle (stateless cycles) or reuse a single long-lived session (persistent context).

## Decision

A single `pi-coding-agent` session is created at loop startup and reused across all cycles. Each cycle builds on the agent's accumulated context from prior cycles via the `lastSummary` carry-forward mechanism.

## Rationale

- **Context accumulation:** Reusing the session allows the agent to remember decisions, file structures, and conventions it has already learned, reducing redundant exploration.
- **Summary bridging:** The `lastSummary` passed to `strategy.nextPrompt()` gives the strategy layer a human-readable handoff between cycles without requiring the consumer to manage raw session state.
- **Simplicity:** A single session is easier to reason about, monitor, and shut down cleanly than a pool of sessions.

## Consequences

- Context windows grow across cycles. Operators must account for this when setting `maxTokensPerCycle`.
- If the agent session is interrupted or corrupted mid-loop, the entire loop must restart (no per-cycle session recovery).
- Strategies that benefit from a "clean slate" must implement that reset behavior themselves within `nextPrompt()` rather than relying on session isolation.

## Trade-off Acknowledged

A stateless-cycles model (fresh session per cycle) would avoid context window growth and provide stronger isolation between cycles. This was rejected in favor of context continuity. If context window growth becomes a practical problem, this decision should be revisited.
