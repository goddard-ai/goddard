import { z } from "zod"
import { ResolvedLoopRateLimits, ResolvedLoopRetries } from "../config.ts"
import { CreateDaemonSessionRequest } from "./sessions.ts"

/** Fully resolved session settings used to start one daemon-owned loop runtime. */
const DaemonLoopSessionConfig = CreateDaemonSessionRequest.omit({
  initialPrompt: true,
  oneShot: true,
})

/** Request payload used to start or reuse one daemon-owned loop runtime. */
export const StartDaemonLoopRequest = z.object({
  rootDir: z.string().min(1),
  loopName: z.string().min(1),
  promptModulePath: z.string().min(1),
  session: DaemonLoopSessionConfig,
  rateLimits: ResolvedLoopRateLimits,
  retries: ResolvedLoopRetries,
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
