import type {
  CreateDaemonSessionRequest,
  DaemonSession,
  GetDaemonSessionHistoryResponse,
  ListDaemonSessionsRequest,
  SessionPromptRequest,
} from "@goddard-ai/sdk"
import { goddardSdk } from "~/sdk.ts"

export type SessionService = {
  createSession(input: CreateDaemonSessionRequest): Promise<DaemonSession>
  listSessions(input: ListDaemonSessionsRequest): Promise<DaemonSession[]>
  getSession(id: DaemonSession["id"]): Promise<DaemonSession>
  getHistory(id: DaemonSession["id"]): Promise<GetDaemonSessionHistoryResponse>
  promptSession(input: SessionPromptRequest): Promise<{ accepted: true }>
}

export const desktopSessionService: SessionService = {
  async createSession(input) {
    const response = await goddardSdk.session.create(input)
    return response.session
  },

  async listSessions(input) {
    const response = await goddardSdk.session.list(input)
    return response.sessions
  },

  async getSession(id) {
    const response = await goddardSdk.session.get({ id })
    return response.session
  },

  async getHistory(id) {
    return await goddardSdk.session.history({ id })
  },

  async promptSession(input) {
    return await goddardSdk.session.prompt(input)
  },
}
