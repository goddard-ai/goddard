# Runtime Loop Semantics

## Overview

`createLoop(config).start()` starts a process that is intended not to return under normal operation.

## Initialization

On creation:
1. Config is validated through `configSchema`.
2. A `RateLimiter` is instantiated from `config.rateLimits`.
3. Mutable runtime status is initialized (`cycle`, `tokensUsed`, `uptime`, internal `startTime`).

On `start()`:
1. The configured model string is resolved against `pi-coding-agent`'s model registry.
2. A persistent `pi-coding-agent` session is created once using resolved model + `projectDir` as cwd.
3. The process enters an infinite cycle loop.

## Per-cycle behavior

For each cycle:
1. Increment cycle counter.
2. Refresh uptime.
3. Apply throttling (`RateLimiter.throttle()`).
4. If `maxCyclesBeforePause` is configured and boundary is reached, sleep for 24h.
5. Build prompt via `strategy.nextPrompt({ cycleNumber, lastSummary })`.
6. Optionally log prompt/cycle if `metrics.enableLogging` is true.
7. Capture pre-prompt total token count.
8. Send prompt to agent session.
9. Read session stats, compute per-cycle token delta, and enforce `maxTokensPerCycle`.
10. Update cumulative `tokensUsed`.
11. Save last assistant text as `lastSummary` (or fallback text).

## Persistent context model

The session instance is intentionally reused across cycles so context accumulates over time.

## Status contract

`loop.status` returns a snapshot with:
- `cycle`
- `tokensUsed`
- `uptime`

`uptime` is recalculated on access.

## Failure model (current)

- Startup/load errors surface as thrown exceptions.
- Unknown/ambiguous configured model values fail fast during startup.
- Exceeding `maxTokensPerCycle` throws and terminates the loop (expected to be handled by external supervisor if desired).
- Runtime loop does not implement internal retry/backoff around agent calls; exceptions will escape unless handled externally by process supervisor.

## Remaining functional gap

- No semantic handling for model outputs like `DONE`.
