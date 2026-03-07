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
      stdio: "inherit",
      env: process.env,
    })

    child.once("error", (error) => reject(error))
    child.once("close", (code, signal) => {
      if (signal) {
        resolve(1)
        return
      }

      resolve(code ?? 1)
    })
  })
}
