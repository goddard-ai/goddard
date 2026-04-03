import { SigmaType } from "preact-sigma"
import type { CreateDaemonSessionRequest, DaemonSession } from "@goddard-ai/sdk"
import type { SessionService } from "./session-service.ts"

type SessionIndexShape = {
  sessionsById: Record<string, DaemonSession>
  orderedSessionIds: string[]
  listStatus: "idle" | "loading" | "ready" | "error"
  errorMessage: string | null
}

export const SessionIndex = new SigmaType<SessionIndexShape>("SessionIndex")
  .defaultState({
    sessionsById: {},
    orderedSessionIds: [],
    listStatus: "idle",
    errorMessage: null,
  })
  .computed({
    sessionList() {
      return this.orderedSessionIds
        .map((sessionId) => this.sessionsById[sessionId])
        .filter((session): session is DaemonSession => Boolean(session))
    },
  })
  .queries({
    getSession(sessionId: DaemonSession["id"]) {
      return this.sessionsById[sessionId] ?? null
    },
  })
  .actions({
    setListLoading() {
      this.listStatus = "loading"
      this.errorMessage = null
    },

    setListError(message: string) {
      this.listStatus = "error"
      this.errorMessage = message
    },

    replaceSessions(sessions: readonly DaemonSession[]) {
      this.sessionsById = Object.fromEntries(sessions.map((session) => [session.id, session]))
      this.orderedSessionIds = sessions.map((session) => session.id)
      this.listStatus = "ready"
      this.errorMessage = null
    },

    upsertSession(session: DaemonSession) {
      this.sessionsById[session.id] = session
      this.orderedSessionIds = [
        session.id,
        ...this.orderedSessionIds.filter((candidateId) => candidateId !== session.id),
      ]
      this.listStatus = "ready"
      this.errorMessage = null
    },

    async refreshSessions(service: SessionService) {
      this.setListLoading()
      this.commit()

      try {
        const sessions = await service.listSessions({ limit: 50 })
        this.replaceSessions(sessions)
        this.commit()
      } catch (error) {
        this.setListError(error instanceof Error ? error.message : String(error))
        this.commit()
      }
    },

    async createSession(service: SessionService, input: CreateDaemonSessionRequest) {
      const session = await service.createSession(input)
      this.upsertSession(session)
      this.commit()
      return session
    },

    async refreshSession(service: SessionService, sessionId: DaemonSession["id"]) {
      try {
        const session = await service.getSession(sessionId)
        this.upsertSession(session)
        this.commit()
        return session
      } catch (error) {
        this.setListError(error instanceof Error ? error.message : String(error))
        this.commit()
        return null
      }
    },
  })

export function lookupSession(sessionIndex: SessionIndex, sessionId: string) {
  return sessionIndex.sessionsById[sessionId] ?? null
}

export interface SessionIndex extends InstanceType<typeof SessionIndex> {}

export type SessionRecord = NonNullable<ReturnType<typeof lookupSession>>
