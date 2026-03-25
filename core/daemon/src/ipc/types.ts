import type {
  AuthSession,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
} from "@goddard-ai/schema/backend"
import type { ReplyPrDaemonRequest, SubmitPrDaemonRequest } from "@goddard-ai/schema/daemon"
import type { ManagedPrLocationRecord } from "../persistence/managed-pr-locations.ts"
import type { SessionPermissionsRecord } from "../persistence/session-permissions.ts"
import type { LoopManager } from "../loop/index.ts"
import type { SessionManager } from "../session/index.ts"
import type { WorkforceManager } from "../workforce/index.ts"

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
  auth: {
    startDeviceFlow: (input?: DeviceFlowStart) => Promise<DeviceFlowSession>
    completeDeviceFlow: (input: DeviceFlowComplete) => Promise<AuthSession>
    whoami: () => Promise<AuthSession>
    logout: () => Promise<void>
  }
  pr: {
    create: (input: PrCreateInput) => Promise<{ number: number; url: string }>
    reply: (input: PrReplyInput) => Promise<{ success: boolean }>
  }
}

export type DaemonServer = {
  daemonUrl: string
  socketPath: string
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
  recordManagedPrLocation?: (
    record: Omit<ManagedPrLocationRecord, "updatedAt">,
  ) => Promise<ManagedPrLocationRecord>
  createLoopManager?: (input: { sessionManager: SessionManager }) => LoopManager
  createWorkforceManager?: (input: { sessionManager: SessionManager }) => WorkforceManager
}
