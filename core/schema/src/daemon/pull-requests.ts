import { z } from "zod"

/** Request payload used to create one pull request through the daemon. */
export const SubmitPrDaemonRequest = z.object({
  cwd: z.string(),
  title: z.string(),
  body: z.string(),
  head: z.string().optional(),
  base: z.string().optional(),
})

export type SubmitPrDaemonRequest = z.infer<typeof SubmitPrDaemonRequest>

/** Request payload used to reply to one pull request through the daemon. */
export const ReplyPrDaemonRequest = z.object({
  cwd: z.string(),
  message: z.string(),
  prNumber: z.number().optional(),
})

export type ReplyPrDaemonRequest = z.infer<typeof ReplyPrDaemonRequest>
