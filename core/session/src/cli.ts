import { command, option, restPositionals, runSafely, string, subcommands } from "cmd-ts"

import { loadEmbeddedPlugin } from "./plugins/registry.ts"
import type { SessionPluginName } from "./plugins/types.ts"

function mergePrompt(parts: string[]): string | undefined {
  const prompt = parts.join(" ").trim()
  return prompt.length > 0 ? prompt : undefined
}

async function runPlugin(name: SessionPluginName, args: { resume?: string; prompt: string[]; argv?: string[] }) {
  const plugin = await loadEmbeddedPlugin(name)

  return await plugin.run(
    {
      resume: args.resume,
      initialPrompt: mergePrompt(args.prompt),
      argv: args.argv,
    },
    {
      cwd: process.cwd(),
      stdin: process.stdin,
      stdout: process.stdout,
      stderr: process.stderr,
    },
  )
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
      return await runPlugin("pi", { resume: args.resume || undefined, prompt: args.prompt })
    },
  })

  const geminiCmd = command({
    name: "gemini",
    args: {
      ...resumeArg(),
      ...promptArgs(),
    },
    handler: async (args) => {
      return await runPlugin("gemini", { resume: args.resume || undefined, prompt: args.prompt })
    },
  })

  const codexCmd = command({
    name: "codex",
    args: {
      ...resumeArg(),
      ...promptArgs(),
    },
    handler: async (args) => {
      return await runPlugin("codex", { resume: args.resume || undefined, prompt: args.prompt })
    },
  })

  const ptyCmd = command({
    name: "pty",
    args: {
      argv: restPositionals({ type: string, displayName: "argv" }),
    },
    handler: async (args) => {
      return await runPlugin("pty", { prompt: [], argv: args.argv })
    },
  })

  const app = subcommands({
    name: "session",
    cmds: {
      pi: piCmd,
      gemini: geminiCmd,
      codex: codexCmd,
      pty: ptyCmd,
    },
  })

  const result = await runSafely(app, argv)
  if (result._tag === "failure") {
    process.stderr.write(`${result.error.message}\n`)
    return result.error.config.exitCode
  }

  if (typeof result.value === "number") {
    return result.value
  }

  if (result.value && typeof (result.value as { value?: number }).value === "number") {
    return (result.value as { value: number }).value
  }

  return 0
}
