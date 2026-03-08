export type SessionPayloadRole = "assistant" | "user" | "system" | "tool"
export type SessionPayloadDriver = "pi" | "pi-rpc" | "gemini" | "codex" | "pty" | "unknown"
export type SessionPayloadFormat = "json-line" | "terminal"

export interface SessionPayloadSource {
  driver: SessionPayloadDriver
  format: SessionPayloadFormat
}

export interface SessionPayloadBase {
  schemaVersion: 1
  source: SessionPayloadSource
  id?: string
  done?: boolean
  raw: unknown
}

export interface SessionPayloadUsage {
  inputTokens?: number
  outputTokens?: number
  totalTokens?: number
}

export interface SessionPayloadToolCall {
  name?: string
  arguments?: unknown
}

export interface SessionPayloadToolResult {
  name?: string
  result?: unknown
}

export interface SessionPayloadCursor {
  x: number
  y: number
}

export interface SessionPayloadTerminalState {
  cols: number
  rows: number
  lines: string[]
  cursor: SessionPayloadCursor
}

export interface DeltaPayload extends SessionPayloadBase {
  kind: "delta"
  role?: SessionPayloadRole
  text?: string
}

export interface MessagePayload extends SessionPayloadBase {
  kind: "message"
  role?: SessionPayloadRole
  text?: string
  message?: string
}

export interface ToolCallPayload extends SessionPayloadBase {
  kind: "tool_call"
  tool?: SessionPayloadToolCall
}

export interface ToolResultPayload extends SessionPayloadBase {
  kind: "tool_result"
  tool?: SessionPayloadToolResult
}

export interface UsagePayload extends SessionPayloadBase {
  kind: "usage"
  usage?: SessionPayloadUsage
}

export interface ErrorPayload extends SessionPayloadBase {
  kind: "error"
  message?: string
}

export interface StatusPayload extends SessionPayloadBase {
  kind: "status"
  message?: string
}

export interface TerminalPayload extends SessionPayloadBase {
  kind: "terminal"
  terminal: SessionPayloadTerminalState
}

export interface UnknownPayload extends SessionPayloadBase {
  kind: "unknown"
}

export type NormalizedSessionPayload =
  | DeltaPayload
  | MessagePayload
  | ToolCallPayload
  | ToolResultPayload
  | UsagePayload
  | ErrorPayload
  | StatusPayload
  | TerminalPayload
  | UnknownPayload
