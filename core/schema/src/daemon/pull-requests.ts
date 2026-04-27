import { z } from "zod"

import { DaemonPullRequestIdParams } from "../common/params.ts"
import { SessionInboxMetadataInput } from "./inbox.ts"
import type { DaemonPullRequest } from "./store.ts"

/** Request payload used to create one pull request through the daemon. */
export const SubmitPrRequest = z.strictObject({
  cwd: z.string(),
  title: z.string(),
  body: z.string(),
  head: z.string().optional(),
  base: z.string().optional(),
  scope: SessionInboxMetadataInput.shape.scope,
  headline: SessionInboxMetadataInput.shape.headline,
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
  scope: SessionInboxMetadataInput.shape.scope,
  headline: SessionInboxMetadataInput.shape.headline,
})

export type ReplyPrRequest = z.infer<typeof ReplyPrRequest>

/** Response payload returned after one pull request reply. */
export type ReplyPrResponse = {
  success: boolean
}

/** Request payload used to fetch one stored daemon pull request by tagged id. */
export const GetPullRequestRequest = DaemonPullRequestIdParams

export type GetPullRequestRequest = z.infer<typeof GetPullRequestRequest>

/** Response payload returned after fetching one stored daemon pull request. */
export type GetPullRequestResponse = {
  pullRequest: DaemonPullRequest
}
