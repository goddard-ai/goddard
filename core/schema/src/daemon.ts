export type DaemonHealth = {
  ok: boolean
}

export type SubmitPrDaemonRequest = {
  cwd: string
  title: string
  body: string
  head?: string
  base?: string
}

export type SubmitPrDaemonResponse = {
  number: number
  url: string
}

export type ReplyPrDaemonRequest = {
  cwd: string
  message: string
  prNumber?: number
}

export type ReplyPrDaemonResponse = {
  success: boolean
}

export type CreateSessionRequest = {
  // Can be expanded as needed later
  config?: any
}

export type Session = {
  id: string // daemon-owned internal session ID (primary key)
  acpId: string // ACP protocol session ID (unique, protocol-facing)
  status: string
  // other properties...
}

export type SessionHistory = {
  events: any[]
}

export type CreateSessionResponse = Session
