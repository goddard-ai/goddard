import { z } from "zod"

/** Tagged daemon session id emitted by the daemon session store. */
export const DaemonSessionId = z.custom<`ses_${string}`>(
  (value): value is `ses_${string}` => typeof value === "string" && value.startsWith("ses_"),
)

export type DaemonSessionId = z.infer<typeof DaemonSessionId>

/** Stable path and payload params used to address one daemon session by id. */
export const DaemonSessionIdParams = z.strictObject({
  id: DaemonSessionId,
})

export type DaemonSessionIdParams = z.infer<typeof DaemonSessionIdParams>

/** Tagged daemon pull request id emitted by the daemon pull request store. */
export const DaemonPullRequestId = z.custom<`pr_${string}`>(
  (value): value is `pr_${string}` => typeof value === "string" && value.startsWith("pr_"),
)

export type DaemonPullRequestId = z.infer<typeof DaemonPullRequestId>

/** Stable path and payload params used to address one daemon pull request by id. */
export const DaemonPullRequestIdParams = z.strictObject({
  id: DaemonPullRequestId,
})

export type DaemonPullRequestIdParams = z.infer<typeof DaemonPullRequestIdParams>
