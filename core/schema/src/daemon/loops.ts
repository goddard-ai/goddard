import { z } from "zod"

import { LoopRateLimits, LoopRetryPolicy } from "../config.ts"
import { CreateSessionRequest } from "./sessions.ts"

/** Runtime loop overrides accepted from a daemon client before local package resolution. */
const LoopSessionOverrides = CreateSessionRequest.omit({
  initialPrompt: true,
  oneShot: true,
}).partial()

/** Request payload used to start or reuse one daemon-owned loop runtime. */
export const StartLoopRequest = z.strictObject({
  rootDir: z.string().min(1),
  loopName: z.string().min(1),
  session: LoopSessionOverrides.optional(),
  rateLimits: LoopRateLimits.optional(),
  retries: LoopRetryPolicy.optional(),
})

/** Type shape for one loop start or reuse request routed over daemon IPC. */
export type StartLoopRequest = z.infer<typeof StartLoopRequest>

/** Request payload used to fetch one daemon-owned loop runtime. */
export const GetLoopRequest = z.strictObject({
  rootDir: z.string().min(1),
  loopName: z.string().min(1),
})

/** Type shape for one loop lookup request routed over daemon IPC. */
export type GetLoopRequest = z.infer<typeof GetLoopRequest>

/** Request payload used to stop one daemon-owned loop runtime. */
export const ShutdownLoopRequest = z.strictObject({
  rootDir: z.string().min(1),
  loopName: z.string().min(1),
})

/** Type shape for one loop shutdown request routed over daemon IPC. */
export type ShutdownLoopRequest = z.infer<typeof ShutdownLoopRequest>

/** Stable runtime states reported for daemon-managed loop hosts. */
export type DaemonLoopRuntimeState = "running"

/** Resolved session and pacing config owned by one daemon-managed loop runtime. */
export type DaemonLoopConfig = {
  promptModulePath: string
  session: Omit<CreateSessionRequest, "initialPrompt" | "oneShot">
  rateLimits: {
    cycleDelay: string
    maxOpsPerMinute: number
    maxCyclesBeforePause: number
  }
  retries: {
    maxAttempts: number
    initialDelayMs: number
    maxDelayMs: number
    backoffFactor: number
    jitterRatio: number
  }
}

/** Loop status summary exposed over daemon IPC. */
export type DaemonLoopStatus = {
  state: DaemonLoopRuntimeState
  rootDir: string
  loopName: string
  promptModulePath: string
  startedAt: string
  sessionId: string
  acpSessionId: string
  cycleCount: number
  lastPromptAt: string | null
}

/** One daemon-managed loop runtime addressed by repository root and loop name. */
export type DaemonLoop = DaemonLoopStatus & DaemonLoopConfig

/** Response payload returned when one loop runtime is fetched. */
export type GetLoopResponse = {
  loop: DaemonLoop
}

/** Response payload returned when one loop runtime is started. */
export type StartLoopResponse = {
  loop: DaemonLoop
}

/** Response payload returned when all running loop runtimes are listed. */
export type ListLoopsResponse = {
  loops: DaemonLoopStatus[]
}

/** Response payload returned after one loop runtime is stopped. */
export type ShutdownLoopResponse = {
  rootDir: string
  loopName: string
  success: boolean
}
