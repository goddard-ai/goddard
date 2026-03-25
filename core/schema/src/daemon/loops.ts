import { z } from "zod"
import { LoopRateLimits, LoopRetryPolicy } from "../config.ts"
import { CreateDaemonSessionRequest } from "./sessions.ts"

/** Runtime loop overrides accepted from a daemon client before local package resolution. */
const DaemonLoopSessionOverrides = CreateDaemonSessionRequest.omit({
  initialPrompt: true,
  oneShot: true,
}).partial()

/** Request payload used to start or reuse one daemon-owned loop runtime. */
export const StartDaemonLoopRequest = z.object({
  rootDir: z.string().min(1),
  loopName: z.string().min(1),
  session: DaemonLoopSessionOverrides.optional(),
  rateLimits: LoopRateLimits.optional(),
  retries: LoopRetryPolicy.optional(),
})

/** Type shape for one loop start or reuse request routed over daemon IPC. */
export type StartDaemonLoopRequest = z.infer<typeof StartDaemonLoopRequest>

/** Request payload used to fetch one daemon-owned loop runtime. */
export const GetDaemonLoopRequest = z.object({
  rootDir: z.string().min(1),
  loopName: z.string().min(1),
})

/** Type shape for one loop lookup request routed over daemon IPC. */
export type GetDaemonLoopRequest = z.infer<typeof GetDaemonLoopRequest>

/** Request payload used to stop one daemon-owned loop runtime. */
export const ShutdownDaemonLoopRequest = GetDaemonLoopRequest

/** Type shape for one loop shutdown request routed over daemon IPC. */
export type ShutdownDaemonLoopRequest = z.infer<typeof ShutdownDaemonLoopRequest>
