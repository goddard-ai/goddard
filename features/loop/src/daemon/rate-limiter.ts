/** Rate limiter that enforces loop cadence and rolling per-minute throughput. */
export class LoopRateLimiter {
  readonly #wallclockDelay: string
  readonly #opsLimit: number
  #opsWindow: number[] = []

  constructor(config: { cycleDelay: string; maxOpsPerMinute: number }) {
    this.#wallclockDelay = config.cycleDelay
    this.#opsLimit = config.maxOpsPerMinute
  }

  /** Sleeps long enough to satisfy both cadence and rolling throughput constraints. */
  async throttle(onPause: (ms: number) => Promise<void>): Promise<void> {
    let delayMs = resolveWallclockDelay(this.#wallclockDelay)
    const now = Date.now()
    this.#opsWindow = this.#opsWindow.filter((time) => now - time < 60_000)

    if (this.#opsWindow.length >= this.#opsLimit) {
      const oldestOp = this.#opsWindow[0] ?? now
      delayMs = Math.max(delayMs, 60_000 - (now - oldestOp))
    }

    if (delayMs > 0) {
      await onPause(delayMs)
    }

    this.#opsWindow.push(Date.now())
  }
}

/** Resolves one persisted cadence expression into a wall-clock delay in milliseconds. */
function resolveWallclockDelay(delay: string): number {
  const match = delay.match(/^(\d+)([smhd])$/)
  if (match) {
    const amount = Number.parseInt(match[1] ?? "0", 10)
    const unit = match[2]
    switch (unit) {
      case "s":
        return amount * 1000
      case "m":
        return amount * 60 * 1000
      case "h":
        return amount * 60 * 60 * 1000
      case "d":
        return amount * 24 * 60 * 60 * 1000
    }
  }

  try {
    const nextRun = Bun.cron.parse(delay)
    return nextRun ? nextRun.getTime() - Date.now() : 60 * 1000
  } catch {
    // Treat invalid cron syntax as a soft configuration error and retry on a safe default cadence.
    return 60 * 1000
  }
}
