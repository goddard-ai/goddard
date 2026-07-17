import type { TerminalSpawnOptions } from "@goddard-ai/schema/daemon/terminals"

type TerminalHostEnvironment = Record<string, string | undefined>

/** Resolves one complete terminal command at the host-platform boundary. */
export function resolveTerminalLaunch(
  options: TerminalSpawnOptions,
  platform: NodeJS.Platform,
  env: TerminalHostEnvironment,
) {
  if (options.command) {
    return {
      command: options.command,
      args: options.args ?? [],
    }
  }

  if (platform === "win32") {
    return {
      command: env.COMSPEC ?? "cmd.exe",
      args: options.args ?? [],
    }
  }

  return {
    command: env.SHELL ?? "/bin/sh",
    args: options.args ?? ["-l"],
  }
}
