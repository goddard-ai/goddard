import type {
  SessionClientEvent,
  SessionDriverCapabilities,
  SessionServerEvent,
  SessionStartupInput,
} from "@goddard-ai/session-protocol"

export type SessionDriverName = "pi" | "pi-rpc" | "gemini" | "codex" | "pty"

export interface SessionDriverInput extends SessionStartupInput {
  initialPrompt?: string
  argv?: string[]
}

/**
 * Unified session driver contract.
 */
export interface SessionDriver {
  readonly name: SessionDriverName

  // Server mode
  start(input: SessionStartupInput): void | Promise<void>
  sendEvent(event: SessionClientEvent): void | Promise<void>
  onEvent(listener: (event: SessionServerEvent) => void): () => void
  getCapabilities?(): SessionDriverCapabilities
  close?(): void
}
