import cronParser from "cron-parser"

export class RateLimiter {
  readonly #wallclockDelay: string
  readonly #opsLimit: number
  #opsWindow: number[] = []

  constructor(config: { cycleDelay: string; maxOpsPerMinute: number }) {
    this.#wallclockDelay = config.cycleDelay
    this.#opsLimit = config.maxOpsPerMinute
  }

  async throttle(onPause?: (ms: number) => Promise<void>): Promise<void> {
    const delayRegex = /^(\d+)([smhd])$/
    const match = this.#wallclockDelay.match(delayRegex)
    let delayMs = 0

    if (match) {
      const amount = Number.parseInt(match[1] ?? "0", 10)
      const unit = match[2]
      switch (unit) {
        case "s":
          delayMs = amount * 1000
          break
        case "m":
          delayMs = amount * 60 * 1000
          break
        case "h":
          delayMs = amount * 60 * 60 * 1000
          break
        case "d":
          delayMs = amount * 24 * 60 * 60 * 1000
          break
      }
    } else {
      try {
        const interval = cronParser.parse(this.#wallclockDelay)
        const next = interval.next().toDate()
        delayMs = next.getTime() - Date.now()
      } catch {
        delayMs = 60 * 1000
      }
    }

    const now = Date.now()
    this.#opsWindow = this.#opsWindow.filter((time) => now - time < 60_000)

    if (this.#opsWindow.length >= this.#opsLimit) {
      const oldestOp = this.#opsWindow[0] ?? now
      const waitTime = 60_000 - (now - oldestOp)
      if (waitTime > delayMs) {
        delayMs = waitTime
      }
    }

    if (delayMs > 0) {
      if (onPause) {
        await onPause(delayMs)
      } else {
        await sleep(delayMs)
      }
    }

    this.#opsWindow.push(Date.now())
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}
