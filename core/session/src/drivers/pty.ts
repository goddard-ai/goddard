import * as pty from "node-pty"

import type { SessionDriver } from "./types.ts"

export const driver: SessionDriver = {
  name: "pty",
  run: async (input, context) => {
    const argv = input.argv ?? []
    if (argv.length === 0) {
      throw new Error("pty driver requires a command, e.g. session pty -- bash")
    }

    const [command, ...args] = argv

    return await new Promise<number>((resolve) => {
      const ptyProcess = pty.spawn(command, args, {
        name: "xterm-color",
        cols: process.stdout.columns || 80,
        rows: process.stdout.rows || 24,
        cwd: context.cwd,
        env: process.env as Record<string, string>,
      })

      const onData = (data: string) => {
        context.stdout.write(data)
      }

      const onStdinData = (data: string | Buffer) => {
        ptyProcess.write(data.toString())
      }

      const onResize = () => {
        ptyProcess.resize(process.stdout.columns || 80, process.stdout.rows || 24)
      }

      ptyProcess.onData(onData)
      context.stdin.on("data", onStdinData)
      process.stdout.on("resize", onResize)

      if (context.stdin.isTTY) {
        context.stdin.setRawMode(true)
      }

      context.stdin.resume()

      ptyProcess.onExit(({ exitCode }) => {
        if (context.stdin.isTTY) {
          context.stdin.setRawMode(false)
        }

        context.stdin.off("data", onStdinData)
        process.stdout.off("resize", onResize)
        resolve(exitCode)
      })
    })
  },
}
