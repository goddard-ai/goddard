import type { NormalizedSessionPayload } from "./payloads.ts"

export type SessionClientEvent =
  | { type: "input.text"; text: string }
  | { type: "input.terminal"; data: string }
  | { type: "terminal.resize"; cols: number; rows: number }

export type SessionServerEvent =
  | { type: "output.text"; text: string }
  | { type: "output.terminal"; data: string }
  | { type: "output.normalized"; payload: NormalizedSessionPayload }
  | { type: "session.exit"; exitCode: number }
  | { type: "session.error"; message: string }

export interface SessionDriverCapabilities {
  terminal: {
    enabled: boolean
    canResize: boolean
    hasScreenState: boolean
  }
  normalizedOutput: boolean
}

export interface SessionTerminalState {
  cols: number
  rows: number
  lines: string[]
  cursor: {
    x: number
    y: number
  }
}

export interface SessionInitializeResult {
  protocolVersion: 1
  driver: string
  capabilities: SessionDriverCapabilities
  state: {
    terminal: SessionTerminalState | null
  }
}

export interface SessionGetStateResult {
  terminal: SessionTerminalState | null
}

export interface SessionEventNotification {
  method: "session_event"
  params: {
    sequence: number
    event: SessionServerEvent
  }
}
