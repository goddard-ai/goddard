import { definePlugin, type DbContext } from "@goddard-ai/daemon-plugin"
import { IpcClientError } from "@goddard-ai/ipc"
import type { SecurityConfig } from "@goddard-ai/schema/config"
import { sessionPlugin } from "@goddard-ai/session/daemon"
import type { DaemonSession } from "@goddard-ai/session/schema"

import { pullRequestBackendRoutes } from "./backend.ts"
import { pullRequestIpcRoutes } from "./daemon-ipc.ts"
import { pullRequestEvents } from "./daemon/events.ts"
import { resolveReplyRequestFromGit, resolveSubmitRequestFromGit } from "./daemon/git.ts"
import { pullRequestDbSchema } from "./daemon/store.ts"
import type { DaemonPullRequest } from "./schema.ts"

export { pullRequestDbSchema } from "./daemon/store.ts"

type PullRequestSessionRecord = {
  sessionId: DaemonSession["id"]
  owner: string | null
  repo: string | null
  allowedPrNumbers: readonly number[]
}
type PullRequestDb = DbContext<typeof pullRequestDbSchema>
type PullRequestRootConfig = {
  security?: SecurityConfig
}

function requireRepositorySession(session: PullRequestSessionRecord | null) {
  if (!session) {
    throw new IpcClientError("Invalid session token")
  }

  if (!session.owner || !session.repo) {
    throw new IpcClientError("Session is not scoped to a repository")
  }

  return session as typeof session & {
    owner: string
    repo: string
  }
}

/** Rejects PR operations disabled by root security policy for the request checkout. */
async function assertPullRequestOperationAllowed(
  configProvider: {
    getRootConfig: (cwd: string) => Promise<{ config: PullRequestRootConfig }>
  },
  cwd: string,
  operation: "submit" | "reply",
) {
  const config = await configProvider.getRootConfig(cwd).then((root) => root.config)
  if (config.security?.pullRequests?.[operation] === "deny") {
    throw new IpcClientError(`Pull request ${operation} is disabled by security policy`)
  }
}

async function recordPullRequest(
  db: PullRequestDb,
  record: Omit<DaemonPullRequest, "id" | "updatedAt">,
) {
  return db.pullRequests.putByUnique(
    {
      host: record.host,
      owner: record.owner,
      repo: record.repo,
      prNumber: record.prNumber,
    },
    record,
  )
}

export const pullRequestPlugin = definePlugin({
  name: "pull-request",
  consumes: [sessionPlugin],
  db: pullRequestDbSchema,
  events: pullRequestEvents,
  backendRoutes: pullRequestBackendRoutes,
  ipcRoutes: pullRequestIpcRoutes,
  setup({ backend, configProvider, db, events, ipc, session }) {
    return {
      ipcHandlers: {
        pr: {
          submit: async ({ body: payload }) => {
            await assertPullRequestOperationAllowed(configProvider, payload.cwd, "submit")
            const sessionRecord = requireRepositorySession(
              await session.resolveTokenScope(payload.token),
            )
            ipc.requestContext.setSessionId(sessionRecord.sessionId)

            const resolvedInput = await resolveSubmitRequestFromGit(payload)
            const pr = await backend.pullRequests.create({
              ...resolvedInput,
              owner: sessionRecord.owner,
              repo: sessionRecord.repo,
            })
            await session.allowPullRequest(sessionRecord.sessionId, pr.number)
            const pullRequest = await recordPullRequest(db, {
              host: "github",
              owner: sessionRecord.owner,
              repo: sessionRecord.repo,
              prNumber: pr.number,
              cwd: payload.cwd,
            })
            const metadata = await session.recordTurnAttentionActivity(sessionRecord.sessionId, {
              scope: payload.scope,
              headline: payload.headline,
              fallbackHeadline: resolvedInput.title,
            })
            events.emit("pull_request.created", {
              pullRequestId: pullRequest.id,
              scope: metadata.scope,
              headline: metadata.headline,
              turnId: metadata.turnId,
            })
            await session.recordSessionResult(
              sessionRecord.sessionId,
              `PR Submitted: ${resolvedInput.title}\n${pr.url}\n\n${resolvedInput.body ?? ""}`,
            )
            return { number: pr.number, url: pr.url }
          },
          get: async ({ body: { id } }) => {
            const pullRequest = db.pullRequests.get(id) ?? null
            if (!pullRequest) {
              throw new IpcClientError("Pull request not found")
            }
            return { pullRequest }
          },
          reply: async ({ body: payload }) => {
            await assertPullRequestOperationAllowed(configProvider, payload.cwd, "reply")
            const sessionRecord = requireRepositorySession(
              await session.resolveTokenScope(payload.token),
            )
            ipc.requestContext.setSessionId(sessionRecord.sessionId)

            const resolvedInput = await resolveReplyRequestFromGit(payload)

            if (!sessionRecord.allowedPrNumbers.includes(resolvedInput.prNumber)) {
              throw new IpcClientError(
                `PR #${resolvedInput.prNumber} is not allowed for this session`,
              )
            }

            const response = await backend.pullRequests.comments.create({
              ...resolvedInput,
              owner: sessionRecord.owner,
              repo: sessionRecord.repo,
            })
            const pullRequest = await recordPullRequest(db, {
              host: "github",
              owner: sessionRecord.owner,
              repo: sessionRecord.repo,
              prNumber: resolvedInput.prNumber,
              cwd: payload.cwd,
            })
            const metadata = await session.recordTurnAttentionActivity(sessionRecord.sessionId, {
              scope: payload.scope,
              headline: payload.headline,
              fallbackHeadline: "PR reply posted",
            })
            events.emit("pull_request.updated", {
              pullRequestId: pullRequest.id,
              scope: metadata.scope,
              headline: metadata.headline,
              turnId: metadata.turnId,
            })
            await session.recordSessionResult(
              sessionRecord.sessionId,
              `PR Reply: ${payload.message}`,
            )
            return response
          },
        },
      },
    }
  },
})
