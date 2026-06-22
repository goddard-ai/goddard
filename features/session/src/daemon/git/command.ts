import { spawn } from "node:child_process"

export type GitCommandResult = {
  status: number
  stdout: string
  stderr: string
}

export type GitRunOptions = {
  stdin?: string | "ignore"
  env?: Record<string, string | undefined>
}

export async function runGitCommand(cwd: string, args: string[], options: GitRunOptions = {}) {
  return await new Promise<GitCommandResult>((resolvePromise, rejectPromise) => {
    const child = spawn("git", args, {
      cwd,
      env: {
        ...process.env,
        ...options.env,
      },
      stdio: ["pipe", "pipe", "pipe"],
    })

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
    child.on("error", rejectPromise)
    child.on("close", (status) => {
      resolvePromise({
        status: status ?? 1,
        stdout,
        stderr,
      })
    })

    if (options.stdin && options.stdin !== "ignore") {
      child.stdin.end(options.stdin)
    } else {
      child.stdin.end()
    }
  })
}
