/** Rate limiter that enforces loop cadence and rolling per-minute throughput. */
export declare class LoopRateLimiter {
    #private;
    constructor(config: {
        cycleDelay: string;
        maxOpsPerMinute: number;
    });
    /** Sleeps long enough to satisfy both cadence and rolling throughput constraints. */
    throttle(onPause: (ms: number) => Promise<void>): Promise<void>;
}
//# sourceMappingURL=rate-limiter.d.ts.map