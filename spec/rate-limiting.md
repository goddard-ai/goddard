---
id: rate-limiting-model
status: ACTIVE
links:
  - type: Extends
    target: spec/configuration.md
  - type: Depends-On
    target: spec/runtime-loop.md
---

# Rate Limiting Model

## Goals

Rate limiting constrains three dimensions of autonomous loop behavior to prevent runaway cost or load:

1. **Cycle cadence** — minimum wall-clock delay between consecutive cycles.
2. **Operation throughput** — maximum operations per rolling 60-second window.
3. **Per-cycle token consumption** — hard cap on tokens consumed in a single agent prompt.

---

## Configuration Inputs

```ts
rateLimits: {
  cycleDelay: string;           // e.g. "30m", "2h", or a cron expression
  maxTokensPerCycle: number;    // hard cap; violation terminates the loop
  maxOpsPerMinute: number;      // sliding-window throughput limit
  maxCyclesBeforePause?: number; // optional: pause for 24h after N cycles
}
```

---

## Delay Interpretation

`cycleDelay` is evaluated in this order:

1. **Duration shorthand** — regex `^\d+[smhd]$` (seconds, minutes, hours, days).
2. **Cron expression** — parsed to determine next fire time.
3. **Fallback** — 60 seconds if neither format matches.

---

## Throughput Limit

A sliding 60-second operations window is maintained in memory. If the number of operations recorded in the active window reaches `maxOpsPerMinute`, the rate limiter waits until the oldest recorded operation expires before allowing the next cycle.

---

## Effective Wait Time

The final sleep duration before each cycle is the **maximum** of:
- Wall-clock delay derived from `cycleDelay`.
- Throughput-derived wait required by `maxOpsPerMinute`.

Both constraints are always respected simultaneously.

---

## Token Limit Enforcement

Token enforcement occurs inside the loop runtime on each cycle:

1. Capture cumulative session token count **before** sending the prompt.
2. Capture cumulative session token count **after** receiving the response.
3. Compute delta = post − pre.
4. If delta > `maxTokensPerCycle` → **throw immediately** and terminate the loop.

This is a **hard-stop** by design. Over-budget cycles are made explicitly visible rather than silently accumulating cost. External supervisors (e.g., `systemd` with `Restart=always`) are the recommended recovery mechanism.

---

## Pause Behavior

When `maxCyclesBeforePause` is set and the cycle count reaches that threshold:
- The loop sleeps for **24 hours** before resuming.
- The cycle counter is **not reset** — it continues incrementing from where it paused.
- This provides a natural daily budget boundary for unattended deployments.
