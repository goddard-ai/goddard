import type {
  DaemonTerminalProcessInput,
  DaemonTerminalProcessService,
} from "@goddard-ai/terminal/daemon"
import type { Platform, SpawnRequest, TaskHost } from "vscode-tasks-engine"

export function createVscodeTaskHost(
  terminal: Pick<DaemonTerminalProcessService, "spawn">,
  platform: Platform,
): TaskHost {
  return {
    spawn(request, sink) {
      const process = terminal.spawn({
        options: resolveTaskTerminalOptions(request, platform),
        onOutput: sink.stdout,
      })

      return {
        exit: process.exit.then(({ exitCode, signal }) => ({
          code: exitCode,
          ...(signal === null ? {} : { signal }),
        })),
        cancel(signal) {
          process.close(signal)
        },
      }
    },
  }
}

export function resolveTaskTerminalOptions(
  request: SpawnRequest,
  platform: Platform,
): DaemonTerminalProcessInput["options"] {
  if (request.kind === "process") {
    return {
      command: request.command,
      args: request.args,
      cwd: request.cwd,
      env: request.env,
      title: request.label,
    }
  }

  const commandLine = [
    request.command,
    ...request.args.map(platform === "windows" ? quoteWindowsShellArg : quotePosixShellArg),
  ].join(" ")
  const executable = request.shell?.executable ?? defaultShellExecutable(platform, request.env)
  const shellArgs = request.shell?.args ?? defaultShellArgs(platform)

  return {
    command: executable,
    args: [...shellArgs, commandLine],
    cwd: request.cwd,
    env: request.env,
    title: request.label,
  }
}

function defaultShellExecutable(platform: Platform, env: Readonly<Record<string, string>>) {
  if (platform === "windows") {
    return env.COMSPEC ?? env.ComSpec ?? process.env.COMSPEC ?? "cmd.exe"
  }
  return env.SHELL ?? process.env.SHELL ?? "/bin/sh"
}

function defaultShellArgs(platform: Platform) {
  return platform === "windows" ? ["/d", "/s", "/c"] : ["-c"]
}

function quotePosixShellArg(value: string) {
  if (/^[A-Za-z0-9_@%+=:,./-]+$/.test(value)) {
    return value
  }
  return `'${value.replaceAll("'", `'"'"'`)}'`
}

function quoteWindowsShellArg(value: string) {
  if (/^[A-Za-z0-9_@%+=:,./\\-]+$/.test(value)) {
    return value
  }
  return `"${value.replaceAll('"', '\\"')}"`
}
