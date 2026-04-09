import { z } from "zod"

/** Request payload used to create one pull request through the daemon. */
export const SubmitPrRequest = z.strictObject({
  cwd: z.string(),
  title: z.string(),
  body: z.string(),
  head: z.string().optional(),
  base: z.string().optional(),
})

export type SubmitPrRequest = z.infer<typeof SubmitPrRequest>

/** Response payload returned after one pull request submission. */
export type SubmitPrResponse = {
  number: number
  url: string
}

/** Request payload used to reply to one pull request through the daemon. */
export const ReplyPrRequest = z.strictObject({
  cwd: z.string(),
  message: z.string(),
  prNumber: z.number().optional(),
})

export type ReplyPrRequest = z.infer<typeof ReplyPrRequest>

/** Response payload returned after one pull request reply. */
export type ReplyPrResponse = {
  success: boolean
}
