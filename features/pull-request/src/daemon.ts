import { definePlugin } from "@goddard-ai/daemon-plugin"
import { inboxPlugin } from "@goddard-ai/inbox/daemon"
import { IpcClientError } from "@goddard-ai/ipc"
import type { DaemonSession } from "@goddard-ai/schema/daemon"
import { sessionPlugin } from "@goddard-ai/session/daemon"

import { db } from "../../../core/daemon/src/persistence/store.ts"
import { pullRequestIpcSchema } from "./daemon-ipc.ts"
import { resolveReplyRequestFromGit, resolveSubmitRequestFromGit } from "./daemon/git.ts"

type PullRequestSessionRecord = {
  sessionId: DaemonSession["id"]
  owner: string | null
  repo: string | null
  allowedPrNumbers: readonly number[]
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

async function recordPullRequest(record: Parameters<typeof db.pullRequests.create>[0]) {
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
  consumes: [sessionPlugin, inboxPlugin],
  ipc: pullRequestIpcSchema,
  setup({
    addAllowedPrToSession,
    backendClient,
    getSessionByToken,
    inbox,
    session,
    setRequestSessionId,
  }) {
    return {
      requestHandlers: {
        "pr.submit": async (payload) => {
          const sessionRecord = requireRepositorySession(await getSessionByToken(payload.token))
          setRequestSessionId(sessionRecord.sessionId)

          const resolvedInput = await resolveSubmitRequestFromGit(payload)
          const pr = await backendClient.pr.create({
            ...resolvedInput,
            owner: sessionRecord.owner,
            repo: sessionRecord.repo,
          })
          await addAllowedPrToSession(sessionRecord.sessionId, pr.number)
          const pullRequest = await recordPullRequest({
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
          inbox.touchInboxItem({
            entityId: pullRequest.id,
            reason: "pull_request.created",
            scope: metadata.scope,
            headline: metadata.headline,
            turnId: metadata.turnId,
          })
          db.sessions.update(sessionRecord.sessionId, {
            status: "done",
            lastAgentMessage: `PR Submitted: ${resolvedInput.title}\n${pr.url}\n\n${
              resolvedInput.body ?? ""
            }`,
          })
          return { number: pr.number, url: pr.url }
        },
        "pr.get": async ({ id }) => {
          const pullRequest = db.pullRequests.get(id) ?? null
          if (!pullRequest) {
            throw new IpcClientError("Pull request not found")
          }
          return { pullRequest }
        },
        "pr.reply": async (payload) => {
          const sessionRecord = requireRepositorySession(await getSessionByToken(payload.token))
          setRequestSessionId(sessionRecord.sessionId)

          const resolvedInput = await resolveReplyRequestFromGit(payload)

          if (!sessionRecord.allowedPrNumbers.includes(resolvedInput.prNumber)) {
            throw new IpcClientError(
              `PR #${resolvedInput.prNumber} is not allowed for this session`,
            )
          }

          const response = await backendClient.pr.reply({
            ...resolvedInput,
            owner: sessionRecord.owner,
            repo: sessionRecord.repo,
          })
          const pullRequest = await recordPullRequest({
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
          inbox.touchInboxItem({
            entityId: pullRequest.id,
            reason: "pull_request.updated",
            scope: metadata.scope,
            headline: metadata.headline,
            turnId: metadata.turnId,
          })
          db.sessions.update(sessionRecord.sessionId, {
            status: "done",
            lastAgentMessage: `PR Reply: ${payload.message}`,
          })
          return response
        },
      },
    }
  },
})
