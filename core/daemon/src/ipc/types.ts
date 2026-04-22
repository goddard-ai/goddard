import type {
  AuthSession,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
} from "@goddard-ai/schema/backend"

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
  port: number
  close: () => Promise<void>
}
