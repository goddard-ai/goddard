import { command, option, restPositionals, run, string, subcommands } from "cmd-ts"
import type { SessionEndpoint } from "@goddard-ai/session-protocol"

import { startClient } from "../client/src/index.ts"
import { loadEmbeddedDriver } from "./drivers/registry.ts"
import type { SessionDriverName } from "./drivers/types.ts"
import { startServer } from "./server.ts"

async function runDriver(
  name: SessionDriverName,
  args: { resume?: string; prompt: string[]; argv?: string[] },
) {
  const driver = await loadEmbeddedDriver(name)

  const server = await startServer({
    driver,
    transport: "ipc",
    startupInput: {
      resume: args.resume,
    },
  })

  return await new Promise<number>((resolve, reject) => {
    let closing = false

    const cleanup = () => {
      process.off("SIGINT", shutdown)
      process.off("SIGTERM", shutdown)
    }

    const shutdown = () => {
      if (closing) {
        return
      }

      closing = true
      cleanup()
      void server.close().then(() => resolve(0), reject)
    }

    process.once("SIGINT", shutdown)
    process.once("SIGTERM", shutdown)
  })
}

function parseEndpoint(url: string): SessionEndpoint {
  if (url.startsWith("ws+unix://")) {
    return {
      kind: "ipc",
      socketPath: url.slice("ws+unix://".length),
      url,
    }
  }

  if (url.startsWith("ws://")) {
    const parsed = new URL(url)
    const port = Number(parsed.port)

    if (!Number.isFinite(port) || port <= 0) {
      throw new Error(`Invalid websocket endpoint: ${url}`)
    }

    return {
      kind: "tcp",
      port,
      url,
    }
  }

  throw new Error(`Unsupported endpoint scheme: ${url}`)
}

async function connectToServer(args: { endpoint: string }) {
  const client = startClient({
    endpoint: parseEndpoint(args.endpoint),
  })

  return await new Promise<number>((resolve) => {
    client.ws.on("close", (code) => resolve(code))
  })
}

function promptArgs() {
  return {
    prompt: restPositionals({
      type: string,
      displayName: "prompt",
    }),
  }
}

function resumeArg() {
  return {
    resume: option({
      type: string,
      long: "resume",
      short: "r",
      defaultValue: () => "",
    }),
  }
}

export async function runSessionCli(argv: string[]): Promise<number> {
  const piCmd = command({
    name: "pi",
    args: {
      ...resumeArg(),
      ...promptArgs(),
    },
    handler: async (args) => {
      return await runDriver("pi", { resume: args.resume || undefined, prompt: args.prompt })
    },
  })

  const geminiCmd = command({
    name: "gemini",
    args: {
      ...resumeArg(),
      ...promptArgs(),
    },
    handler: async (args) => {
      return await runDriver("gemini", { resume: args.resume || undefined, prompt: args.prompt })
    },
  })

  const codexCmd = command({
    name: "codex",
    args: {
      ...resumeArg(),
      ...promptArgs(),
    },
    handler: async (args) => {
      return await runDriver("codex", { resume: args.resume || undefined, prompt: args.prompt })
    },
  })

  const ptyCmd = command({
    name: "pty",
    args: {
      argv: restPositionals({ type: string, displayName: "argv" }),
    },
    handler: async (args) => {
      return await runDriver("pty", { prompt: [], argv: args.argv })
    },
  })

  const connectCmd = command({
    name: "connect",
    args: {
      endpoint: option({
        type: string,
        long: "endpoint",
      }),
    },
    handler: async (args) => {
      return await connectToServer({
        endpoint: args.endpoint,
      })
    },
  })

  const app = subcommands({
    name: "session",
    cmds: {
      pi: piCmd,
      gemini: geminiCmd,
      codex: codexCmd,
      pty: ptyCmd,
      connect: connectCmd,
    },
  })

  let result
  try {
    result = await run(app, argv)
  } catch (error: any) {
    process.stderr.write(`${error?.message || "Unknown error"}\n`)
    return error?.config?.exitCode || 1
  }

  const value = await result.value

  if (typeof value === "number") {
    return value
  }

  if (value && typeof (value as { value?: number }).value === "number") {
    return (value as { value: number }).value
  }

  return 0
}
