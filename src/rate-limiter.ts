import pLimit from 'p-limit';
import cronParser from 'cron-parser';

export class RateLimiter {
  private tokenLimiter: any;
  private wallclockDelay: string;
  private opsLimit: number;
  private opsWindow: number[] = [];
  private maxTokensPerCycle: number;

  constructor(config: { cycleDelay: string; maxTokensPerCycle: number; maxOpsPerMinute: number }) {
    this.tokenLimiter = pLimit(1); // Not really used in this simplified version since loop is sequential
    this.wallclockDelay = config.cycleDelay;
    this.opsLimit = config.maxOpsPerMinute;
    this.maxTokensPerCycle = config.maxTokensPerCycle;
  }

  async throttle() {
    // Basic throttling based on cycleDelay and opsPerMinute
    const delayRegex = /^(\d+)([smhd])$/;
    const match = this.wallclockDelay.match(delayRegex);
    let delayMs = 0;

    if (match) {
      const amount = parseInt(match[1]);
      const unit = match[2];
      switch (unit) {
        case 's': delayMs = amount * 1000; break;
        case 'm': delayMs = amount * 60 * 1000; break;
        case 'h': delayMs = amount * 60 * 60 * 1000; break;
        case 'd': delayMs = amount * 24 * 60 * 60 * 1000; break;
      }
    } else {
      // Try parsing as cron if not simple delay
      try {
        const interval = cronParser.parseExpression(this.wallclockDelay);
        const next = interval.next().toDate();
        delayMs = next.getTime() - Date.now();
      } catch (e) {
        // Fallback to 1 minute
        delayMs = 60 * 1000;
      }
    }

    // Check ops limit
    const now = Date.now();
    this.opsWindow = this.opsWindow.filter(t => now - t < 60000); // Keep ops from last minute

    if (this.opsWindow.length >= this.opsLimit) {
      // Need to wait until oldest op falls out of window
      const oldestOp = this.opsWindow[0];
      const waitTime = 60000 - (now - oldestOp);
      if (waitTime > delayMs) {
        delayMs = waitTime;
      }
    }

    // Simple sleep implementation for throttling
    if (delayMs > 0) {
      await new Promise(resolve => setTimeout(resolve, delayMs));
    }

    // Record op
    this.opsWindow.push(Date.now());
  }
}
