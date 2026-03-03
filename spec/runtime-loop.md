---
id: runtime-loop-semantics
status: ACTIVE
links:
  - type: Extends
    target: spec/vision.md
  - type: Depends-On
    target: spec/configuration.md
  - type: Depends-On
    target: spec/rate-limiting.md
  - type: Relates-To
    target: spec/cli/loop.md
---

# Runtime Loop Semantics

## Overview

`createLoop(config).start()` enters a process that is intended not to return under normal operation. The loop is designed to be supervised externally (e.g., `systemd`) rather than self-healing internally.

---

## Initialization

**On construction:**
1. Config is validated through `configSchema` (Zod).
2. A `RateLimiter` is instantiated from `config.rateLimits`.
3. Mutable runtime status is initialized: `cycle`, `tokensUsed`, `uptime`, internal `startTime`.

**On `start()`:**
1. The configured model string is resolved against `pi-coding-agent`'s model registry.
2. A persistent `pi-coding-agent` session is created once using the resolved model and `projectDir` as cwd.
3. The process enters the cycle loop.

---

## Per-Cycle Behavior

For each cycle:

1. Increment cycle counter.
2. Refresh uptime.
3. Apply throttling via `RateLimiter.throttle()`.
4. If `maxCyclesBeforePause` is configured and the boundary is reached, sleep for 24 h.
5. Build prompt via `strategy.nextPrompt({ cycleNumber, lastSummary })`.
6. Optionally log prompt/cycle info if `metrics.enableLogging` is `true`.
7. Capture pre-prompt cumulative token count.
8. Send prompt to agent session (with optional retry + exponential backoff + jitter; `retryableErrors` can short-circuit retries).
9. Read session stats, compute per-cycle token delta, and enforce `maxTokensPerCycle`.
10. Update cumulative `tokensUsed`.
11. Save last assistant text as `lastSummary` (or a fallback string).
12. If assistant output indicates `DONE` (exact string `DONE`, format `SUMMARY|DONE`, or trailing line `DONE`), terminate loop successfully.

---

## Persistent Context Model

The `pi-coding-agent` session is intentionally reused across cycles so that context accumulates over time. Each prompt builds on the agent's accumulated knowledge of the codebase and prior cycles via `lastSummary`. This is a deliberate design choice — see [`adr/003-persistent-agent-session.md`](./adr/003-persistent-agent-session.md).

---

## Status Contract

`loop.status` returns a point-in-time snapshot:

```ts
{
  cycle: number;       // cycles completed so far
  tokensUsed: number;  // cumulative token delta across all cycles
  uptime: number;      // milliseconds since loop start (recalculated on access)
}
```

---

## Failure Model

| Condition | Behavior |
|-----------|----------|
| Invalid config on startup | Throws immediately (Zod validation). |
| Unknown model value | Fails fast during startup before first cycle. |
| `maxTokensPerCycle` exceeded | Throws and terminates loop. External supervisor should handle restart. |
| `DONE` in agent output | Clean, successful loop termination with exit code `0`. |
| Transient agent errors | Retried with exponential backoff + jitter if `retries` is configured. |
| Non-retryable errors | `retryableErrors` function classifies; escapes immediately without retry. |
| Max retries exhausted | Error propagates; external supervisor (e.g., `systemd`) handles restart. |

Errors that escape the loop are intentionally unhandled inside the runtime. External process managers are the recommended supervisory mechanism — this is a deliberate architectural boundary, not an oversight.
