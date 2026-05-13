import type { Handlers } from "@goddard-ai/ipc"
import type { DaemonSessionId } from "@goddard-ai/schema/common/params"
import type { SendSessionMessageRequest } from "@goddard-ai/schema/daemon"

import type { sessionIpcSchema } from "../daemon-ipc.ts"
import type { createSessionExtension } from "./extension.ts"

type SessionExtension = ReturnType<typeof createSessionExtension>

type SessionHandlerInput = {
  session: SessionExtension
  setRequestSessionId: (id: DaemonSessionId) => void
}

/** Builds the session daemon IPC handlers contributed by the session feature plugin. */
export function createSessionRequestHandlers(input: SessionHandlerInput) {
  return {
    "session.create": async (payload) => {
      const response = {
        session: await input.session.create(payload),
      }
      input.setRequestSessionId(response.session.id)
      return response
    },
    "session.list": async (payload) => input.session.list(payload),
    "session.get": async ({ id }) => ({
      session: await input.session.get(id),
    }),
    "session.connect": async ({ id }) => ({
      session: await input.session.connect(id),
    }),
    "session.history": async (payload) => input.session.history(payload),
    "session.changes": async ({ id }) => input.session.changes(id),
    "session.composerSuggestions": async (payload) => input.session.composerSuggestions(payload),
    "session.draftSuggestions": async (payload) => input.session.draftSuggestions(payload),
    "session.launchPreview": async (payload) => input.session.launchPreview(payload),
    "session.subpackages": async (payload) => input.session.subpackages(payload),
    "session.diagnostics": async ({ id }) => input.session.diagnostics(id),
    "session.worktree.get": async ({ id }) => input.session.worktree(id),
    "session.reviewSession.mount": async ({ id }) => input.session.mountReviewSession(id),
    "session.reviewSession.run": async ({ id }) => input.session.runReviewSession(id),
    "session.reviewSession.unmount": async ({ id }) => input.session.unmountReviewSession(id),
    "session.workforce.get": async ({ id }) => input.session.workforce(id),
    "session.shutdown": async ({ id }) => ({
      id,
      success: await input.session.shutdown(id),
    }),
    "session.cancel": async ({ id }) => input.session.cancel(id),
    "session.steer": async ({ id, prompt }) => input.session.steer(id, prompt),
    "session.send": async ({ id, message }) => {
      await input.session.sendMessage(id, message as SendSessionMessageRequest["message"])
      return { accepted: true as const }
    },
    "session.complete": async ({ id }) => ({
      item: await input.session.complete(id),
    }),
    "session.declareInitiative": async ({ id, title }) => ({
      session: await input.session.declareInitiative(id, title),
    }),
    "session.reportBlocker": async ({ id, reason, scope, headline }) => ({
      session: await input.session.reportBlocker(id, reason, { scope, headline }),
    }),
    "session.reportTurnEnded": async ({ id, scope, headline }) => ({
      session: await input.session.reportTurnEnded(id, { scope, headline }),
    }),
    "session.resolveToken": async ({ token }) => {
      const id = await input.session.resolveToken(token)
      input.setRequestSessionId(id)
      return {
        id,
      }
    },
  } satisfies Handlers<typeof sessionIpcSchema>
}
