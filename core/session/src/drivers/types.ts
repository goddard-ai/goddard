import type {
  SessionClientEvent,
  SessionDriverCapabilities,
  SessionServerEvent,
} from "@goddard-ai/session-protocol"

export type SessionDriverName = "pi" | "pi-rpc" | "gemini" | "codex" | "pty"

export interface SessionDriverInput {
  resume?: string
  initialPrompt?: string
  argv?: string[]
}

export interface SessionDriverContext {
  cwd: string
  stdin: NodeJS.ReadStream
  stdout: NodeJS.WriteStream
  stderr: NodeJS.WriteStream
}

/**
 * Unified session driver contract.
 *
 * Drivers can implement either (or both) modes:
 * - CLI mode via run(...)
 * - RPC server mode via sendEvent/onEvent/getCapabilities/close
 */
export interface SessionDriver {
  readonly name: SessionDriverName

  // CLI mode
  run?(input: SessionDriverInput, context: SessionDriverContext): Promise<number>

  // Server mode
  sendEvent?(event: SessionClientEvent): void | Promise<void>
  onEvent?(listener: (event: SessionServerEvent) => void): () => void
  getCapabilities?(): SessionDriverCapabilities
  close?(): void
}
