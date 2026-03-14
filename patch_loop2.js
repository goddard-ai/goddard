const fs = require('fs');

let index = fs.readFileSync('core/loop/src/index.ts', 'utf8');

index = index.replace(
  `const limiter = new RateLimiter({
    cycleDelay: "30m",
    maxTokensPerCycle: 128000,
    maxOpsPerMinute: 120,
    maxCyclesBeforePause: 100,
  });`,
  `const limiter = new RateLimiter({
    cycleDelay: "30m",
    maxOpsPerMinute: 120,
  });`
);

index = index.replace(
  `            const isRetryable = retryConfig.retryableErrors
              ? retryConfig.retryableErrors(error, {
                  cycle: status.cycle,
                  attempt,
                  maxAttempts: retryConfig.maxAttempts
                })
              : true;`,
  `            const isRetryable = true;`
);

fs.writeFileSync('core/loop/src/index.ts', index);
