export type SessionDriverName = "pi" | "gemini" | "codex" | "pty"

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

export type SessionDriverDataListener = (data: string) => void

/**
 * Unified session driver contract.
 *
 * Drivers can implement either (or both) modes:
 * - CLI mode via run(...)
 * - RPC server mode via writeInput/onData/close
 */
export interface SessionDriver {
  readonly name: SessionDriverName

  // CLI mode
  run?(input: SessionDriverInput, context: SessionDriverContext): Promise<number>

  // Server mode
  writeInput?(key: string): void
  onData?(listener: SessionDriverDataListener): () => void
  close?(): void
}
