import { z } from "zod"
import { RepoPrRef, RepoRef } from "../common/repository.js"

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
