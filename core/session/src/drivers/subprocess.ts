import { spawn } from "node:child_process"

import type { SessionDriverContext } from "./types.ts"

export async function runSubprocess(
  command: string,
  args: string[],
  context: SessionDriverContext,
): Promise<number> {
  return await new Promise<number>((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: context.cwd,
      stdio: ["pipe", "pipe", "pipe"],
      env: process.env,
    })

    const onStdinData = (data: string | Buffer) => {
      if (!child.stdin.destroyed) {
        child.stdin.write(data)
      }
    }

    context.stdin.on("data", onStdinData)
    context.stdin.resume()
    child.stdout.on("data", (data: Buffer) => context.stdout.write(data))
    child.stderr.on("data", (data: Buffer) => context.stderr.write(data))

    child.once("error", (error) => reject(error))
    child.once("close", (code, signal) => {
      context.stdin.off("data", onStdinData)

      if (signal) {
        resolve(1)
        return
      }

      resolve(code ?? 1)
    })
  })
}
