import type { ReplyPrDaemonRequest, SubmitPrDaemonRequest } from "@goddard-ai/schema/daemon"
import type { SessionPermissionsRecord } from "@goddard-ai/storage/session-permissions"

export type { ReplyPrDaemonRequest, SubmitPrDaemonRequest }

export type PrCreateInput = {
  owner: string
  repo: string
  title: string
  body?: string
  head: string
  base: string
}

export type PrReplyInput = {
  owner: string
  repo: string
  prNumber: number
  body: string
}

export type BackendPrClient = {
  pr: {
    create: (input: PrCreateInput) => Promise<{ number: number; url: string }>
    reply: (input: PrReplyInput) => Promise<{ success: boolean }>
  }
}

export type DaemonServer = {
  daemonUrl: string
  socketPath: string
  sessionManager: {
    shutdownSession: (id: string) => Promise<boolean>
  }
  close: () => Promise<void>
}

export type AuthorizedSession = Pick<
  SessionPermissionsRecord,
  "sessionId" | "owner" | "repo" | "allowedPrNumbers"
>

export type DaemonServerDeps = {
  resolveSubmitRequest?: (input: SubmitPrDaemonRequest) => Promise<PrCreateInput>
  resolveReplyRequest?: (input: ReplyPrDaemonRequest) => Promise<PrReplyInput>
  getSessionByToken?: (token: string) => Promise<AuthorizedSession | null>
  addAllowedPrToSession?: (sessionId: string, prNumber: number) => Promise<void>
}
