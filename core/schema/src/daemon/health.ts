import { z } from "zod"

/** Request payload used to probe daemon liveness. */
export const GetDaemonHealthRequest = z.object({})

export type GetDaemonHealthRequest = z.infer<typeof GetDaemonHealthRequest>

/** Minimal daemon liveness payload returned by health probes. */
export type DaemonHealth = {
  ok: boolean
}
