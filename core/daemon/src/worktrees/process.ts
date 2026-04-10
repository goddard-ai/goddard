/** Async subprocess helpers shared by daemon-owned worktree integrations. */
import { spawn } from "node:child_process"

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
export function runCommand(
  command: string,
  args: string[],
  options: {
    cwd?: string
    stdin?: "ignore" | string
  } = {},
) {
  return new Promise<CommandResult>((resolve, reject) => {
    const child = spawn(command, args, {
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
