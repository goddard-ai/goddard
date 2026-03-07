export type SessionPluginName = "pi" | "gemini" | "codex" | "pty"

export interface SessionPluginInput {
  resume?: string
  initialPrompt?: string
  argv?: string[]
}

export interface SessionPluginContext {
  cwd: string
  stdin: NodeJS.ReadStream
  stdout: NodeJS.WriteStream
  stderr: NodeJS.WriteStream
}

export interface SessionPlugin {
  readonly name: SessionPluginName
  run(input: SessionPluginInput, context: SessionPluginContext): Promise<number>
}
