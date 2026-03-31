import { spawn, type IPty, type IPtyForkOptions } from "bun-pty"

const DEFAULT_TERMINAL_NAME = "xterm-256color"

export type TerminalPtyOptions = {
  command?: string
  args?: string[]
  cwd?: string
  env?: Record<string, string>
  cols?: number
  rows?: number
  name?: string
}

/** Spawns one Bun-hosted PTY suitable for future terminal tab wiring. */
export function spawnTerminalPty(options: TerminalPtyOptions = {}): IPty {
  const spawnOptions: IPtyForkOptions = {
    name: options.name ?? DEFAULT_TERMINAL_NAME,
    cols: options.cols ?? 80,
    rows: options.rows ?? 24,
    cwd: options.cwd,
    env: options.env,
  }

  return spawn(
    resolveShellCommand(options.command),
    options.args ?? defaultShellArgs(),
    spawnOptions,
  )
}

function resolveShellCommand(command?: string): string {
  return command ?? Bun.env.SHELL ?? "/bin/zsh"
}

function defaultShellArgs(): string[] {
  return ["-l"]
}
