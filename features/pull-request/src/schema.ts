import { RepoRef } from "@goddard-ai/remote-repo/schema"
import { AttentionMetadataInput } from "@goddard-ai/schema/attention"
import { z } from "zod"

const RepoPrRef = RepoRef.extend({
  prNumber: z.number(),
})

/** Tagged pull request id emitted by the pull-request store. */
export const PullRequestId = z.custom<`pr_${string}`>(
  (value): value is `pr_${string}` => typeof value === "string" && value.startsWith("pr_"),
)

export type PullRequestId = z.infer<typeof PullRequestId>

/** Stable path and payload params used to address one pull request by id. */
export const PullRequestIdParams = z.strictObject({
  id: PullRequestId,
})

export type PullRequestIdParams = z.infer<typeof PullRequestIdParams>

/** Request payload used to create one managed pull request. */
export const CreatePrInput = RepoRef.extend({
  title: z.string(),
  body: z.string().optional(),
  head: z.string(),
  base: z.string(),
})

export type CreatePrInput = z.infer<typeof CreatePrInput>

/** Request payload used to reply to one managed pull request. */
export const ReplyPrInput = RepoPrRef.extend({
  body: z.string(),
})

export type ReplyPrInput = z.infer<typeof ReplyPrInput>

/** Query payload used to check whether one pull request is managed. */
export const ManagedPrQuery = RepoRef.extend({
  prNumber: z.coerce.number(),
})

export type ManagedPrQuery = z.infer<typeof ManagedPrQuery>

/** Persistent backend record describing one managed pull request. */
export const PullRequestRecord = z.object({
  id: z.number(),
  number: z.number(),
  provider: z.string(),
  owner: z.string(),
  repo: z.string(),
  title: z.string(),
  body: z.string(),
  head: z.string(),
  base: z.string(),
  url: z.string(),
  createdBy: z.string(),
  createdAt: z.string(),
})

export type PullRequestRecord = z.infer<typeof PullRequestRecord>

/** Persisted daemon-managed pull request record owned by the pull-request feature. */
export const DaemonPullRequest = z.strictObject({
  host: z.enum(["github"]),
  owner: z.string(),
  repo: z.string(),
  prNumber: z.number().int(),
  cwd: z.string(),
})

export type DaemonPullRequest = z.output<typeof DaemonPullRequest> & {
  id: PullRequestId
  updatedAt: number
}

/** Request payload used to create one pull request through the daemon. */
export const SubmitPrRequest = z.strictObject({
  cwd: z.string(),
  title: z.string(),
  body: z.string(),
  head: z.string().optional(),
  base: z.string().optional(),
  scope: AttentionMetadataInput.shape.scope,
  headline: AttentionMetadataInput.shape.headline,
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
  scope: AttentionMetadataInput.shape.scope,
  headline: AttentionMetadataInput.shape.headline,
})

export type ReplyPrRequest = z.infer<typeof ReplyPrRequest>

/** Response payload returned after one pull request reply. */
export type ReplyPrResponse = {
  success: boolean
}

/** Request payload used to fetch one stored daemon pull request by tagged id. */
export const GetPullRequestRequest = PullRequestIdParams

export type GetPullRequestRequest = z.infer<typeof GetPullRequestRequest>

/** Response payload returned after fetching one stored daemon pull request. */
export type GetPullRequestResponse = {
  pullRequest: DaemonPullRequest
}
