import {
  sessionServerEventSchema,
  type SessionClientEvent,
  type SessionDriverCapabilities,
  type SessionServerEvent,
  type SessionStartupInput,
} from "@goddard-ai/session-protocol"

export type SessionDriverName = "pi" | "gemini" | "codex" | "pty"

export interface SessionDriverInput extends SessionStartupInput {
  initialPrompt?: string
  argv?: string[]
}

/**
 * Unified session driver base class.
 */
export abstract class SessionDriver {
  abstract readonly name: SessionDriverName

  private readonly listeners = new Set<(event: SessionServerEvent) => void>()

  // Server mode
  abstract start(input: SessionStartupInput): void | Promise<void>
  abstract sendEvent(event: SessionClientEvent): void | Promise<void>

  onEvent(listener: (event: SessionServerEvent) => void): () => void {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  emit(event: SessionServerEvent) {
    const parsed = sessionServerEventSchema.parse(event)
    for (const listener of this.listeners) {
      listener(parsed)
    }
  }

  getCapabilities?(): SessionDriverCapabilities
  close?(): void
}
