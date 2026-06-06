/** Async subprocess helpers shared by session-owned worktree integrations. */
import { spawn } from "node:child_process"
import { constants as fsConstants } from "node:fs"
import { access } from "node:fs/promises"
import { delimiter, extname, isAbsolute, join } from "node:path"

/**
 * Minimal result shape returned by async subprocess helpers in this package.
 */
export interface CommandResult {
  status: number | null
  stdout: string
  stderr: string
}

/**
 * Spawns one subprocess without blocking the event loop and captures text output.
 */
export async function runCommand(
  command: string,
  args: string[],
  options: {
    cwd?: string
    stdin?: "ignore" | string
  } = {},
) {
  const spawnSpec = await resolveSpawnSpec(command, args)

  return new Promise<CommandResult>((resolve, reject) => {
    const child = spawn(spawnSpec.command, spawnSpec.args, {
      cwd: options.cwd,
      stdio: [options.stdin === "ignore" ? "ignore" : "pipe", "pipe", "pipe"],
    })

    if (!child.stdout || !child.stderr) {
      reject(new Error(`Failed to capture output for command: ${command}`))
      return
    }

    if (typeof options.stdin === "string") {
      child.stdin?.end(options.stdin)
    }

    let stdout = ""
    let stderr = ""

    child.stdout.setEncoding("utf8")
    child.stderr.setEncoding("utf8")
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk
    })
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk
    })

    child.on("error", reject)
    child.on("close", (status: number | null) => {
      resolve({ status, stdout, stderr })
    })
  })
}

type SpawnSpec = {
  command: string
  args: string[]
}

async function resolveSpawnSpec(command: string, args: string[]): Promise<SpawnSpec> {
  if (process.platform !== "win32") {
    return { command, args }
  }

  const resolvedCommand = await resolveWindowsCommand(command)
  if (!resolvedCommand || !isWindowsBatchCommand(resolvedCommand)) {
    return { command: resolvedCommand ?? command, args }
  }

  return {
    command: process.env.ComSpec || "cmd.exe",
    args: ["/d", "/s", "/c", quoteCmdCommand([resolvedCommand, ...args])],
  }
}

async function resolveWindowsCommand(command: string) {
  for (const candidate of getWindowsCommandCandidates(command)) {
    try {
      await access(candidate, fsConstants.X_OK)
      return candidate
    } catch {
      // Keep searching PATH/PATHEXT candidates.
    }
  }

  return null
}

function getWindowsCommandCandidates(command: string) {
  if (hasPathSeparator(command) || isAbsolute(command)) {
    return getWindowsExtensionCandidates(command)
  }

  const pathDirs = (process.env.PATH || "").split(delimiter).filter((entry) => entry.length > 0)
  return pathDirs.flatMap((dir) => getWindowsExtensionCandidates(join(dir, command)))
}

function getWindowsExtensionCandidates(commandPath: string) {
  if (extname(commandPath)) {
    return [commandPath]
  }

  return getWindowsPathExtensions().map((extension) => `${commandPath}${extension}`)
}

function getWindowsPathExtensions() {
  return (process.env.PATHEXT || ".COM;.EXE;.BAT;.CMD")
    .split(";")
    .filter((extension) => extension.length > 0)
}

function hasPathSeparator(value: string) {
  return value.includes("/") || value.includes("\\")
}

function isWindowsBatchCommand(command: string) {
  const extension = extname(command).toLowerCase()
  return extension === ".bat" || extension === ".cmd"
}

function quoteCmdCommand(commandAndArgs: string[]) {
  return commandAndArgs.map(quoteCmdArg).join(" ")
}

function quoteCmdArg(value: string) {
  if (!/[()\s"&^<>|]/.test(value)) {
    return value
  }

  return `"${value.replace(/"/g, '\\"')}"`
}
