# Rate Limiting Model

## Goals

Rate limiting exists to constrain loop cadence, per-minute operation throughput, and per-cycle token consumption.

## Inputs

```ts
rateLimits: {
  cycleDelay: string;        // e.g. "30m" or cron expression
  maxTokensPerCycle: number;
  maxOpsPerMinute: number;
}
```

## Delay interpretation

`cycleDelay` is interpreted in this order:
1. Duration shorthand: `^\d+[smhd]$`
2. Cron expression via `cron-parser`
3. Fallback to 60s if parsing fails

## Throughput limit

A sliding 60-second operations window is maintained.

If operation count in the active window reaches `maxOpsPerMinute`, the limiter waits until the oldest operation expires from the window.

## Effective wait time

The final sleep duration is the maximum of:
- wall-clock delay derived from `cycleDelay`
- throughput-derived wait required by `maxOpsPerMinute`

## Token limit enforcement

Token enforcement occurs in the loop runtime:
1. capture total session tokens before prompt,
2. capture total session tokens after prompt,
3. compute delta for the cycle,
4. throw if delta exceeds `maxTokensPerCycle`.

This is an immediate hard-stop behavior, intended to make over-budget cycles explicit.
