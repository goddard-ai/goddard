import type { KindInput, KindOutput } from "kindstore"
import type {
  AuthSession,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
} from "@goddard-ai/schema/backend"
import type {
  DaemonSession,
  DaemonWorkforceEvent,
  ReplyPrDaemonRequest,
  SubmitPrDaemonRequest,
} from "@goddard-ai/schema/daemon"
import { db } from "../persistence/store.ts"
import type { ConfigManager } from "../config-manager.ts"
import type { LoopManager } from "../loop/index.ts"
import type { LoopManagerDeps } from "../loop/manager.ts"
import type { SessionManager } from "../session/index.ts"
import type { WorkforceManager } from "../workforce/index.ts"

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

export type AuthorizedSession = {
  sessionId: DaemonSession["id"]
  owner: string
  repo: string
  allowedPrNumbers: number[]
}

export type DaemonServerDeps = {
  resolveSubmitRequest?: (input: SubmitPrDaemonRequest) => Promise<PrCreateInput>
  resolveReplyRequest?: (input: ReplyPrDaemonRequest) => Promise<PrReplyInput>
  getSessionByToken?: (token: string) => Promise<AuthorizedSession | null>
  addAllowedPrToSession?: (sessionId: DaemonSession["id"], prNumber: number) => Promise<void>
  recordPullRequest?: (
    record: KindInput<typeof db.schema.pullRequests>,
  ) => Promise<KindOutput<typeof db.schema.pullRequests>>
  configManager?: ConfigManager
  createLoopManager?: (input: LoopManagerDeps) => LoopManager
  createWorkforceManager?: (input: {
    sessionManager: SessionManager
    publishEvent?: (payload: DaemonWorkforceEvent) => void
  }) => WorkforceManager
}
