#!/usr/bin/env node
import { command, flag, option, run, string } from "cmd-ts"
import { readFile } from "node:fs/promises"
import { createRequire } from "node:module"
import type { RegistryAgent } from "../registry.js"

type AgentDistribution = RegistryAgent["distribution"]

export type DemoOptions = {
  enableAuth: boolean
  cwd: string
  prompt: string
  agent?: string | AgentDistribution
}

function isAgentDistribution(value: unknown): value is AgentDistribution {
  if (!value || typeof value !== "object") {
    return false
  }

  const candidate = value as {
    type?: unknown
    cmd?: unknown
    package?: unknown
    args?: unknown
  }

  if (candidate.type !== "binary" && candidate.type !== "npx" && candidate.type !== "uvx") {
    return false
  }

  if (candidate.type === "binary") {
    if (typeof candidate.cmd !== "string") {
      return false
    }
    if (candidate.args != null && !Array.isArray(candidate.args)) {
      return false
    }
    return true
  }

  return typeof candidate.package === "string"
}

export function parseAgentOption(value: string): string | AgentDistribution {
  const trimmed = value.trim()
  if (!trimmed.startsWith("{")) {
    return trimmed
  }

  const parsed = JSON.parse(trimmed)
  if (!isAgentDistribution(parsed)) {
    throw new Error("--agent JSON must match AgentDistribution")
  }

  return parsed
}

export async function parseAgentFileOption(path: string): Promise<AgentDistribution> {
  const fileContent = await readFile(path, "utf8")
  const parsed = JSON.parse(fileContent)
  if (!isAgentDistribution(parsed)) {
    throw new Error("--agent-file JSON must match AgentDistribution")
  }
  return parsed
}

const demoArgsCommand = command({
  name: "client-demo",
  description: "Run the @goddard-ai/session client demo",
  args: {
    enableAuth: flag({
      long: "enable-auth",
      description: "Use a real registry agent (claude-code) instead of the local demo agent",
    }),
    agent: option({
      type: string,
      long: "agent",
      description: "Registry name or JSON AgentDistribution",
      defaultValue: () => "",
    }),
    agentFile: option({
      type: string,
      long: "agent-file",
      description: "Path to JSON AgentDistribution file",
      defaultValue: () => "",
    }),
    cwd: option({
      type: string,
      long: "cwd",
      description: "Working directory for the session",
      defaultValue: () => process.cwd(),
    }),
    prompt: option({
      type: string,
      long: "prompt",
      description: "Prompt to send after startup",
      defaultValue: () => "Summarize what you can do in this demo session.",
    }),
  },
  handler: async (args): Promise<DemoOptions> => {
    if (args.agent && args.agentFile) {
      throw new Error("--agent and --agent-file are mutually exclusive")
    }

    let agent: DemoOptions["agent"]
    if (args.agentFile) {
      agent = await parseAgentFileOption(args.agentFile)
    } else if (args.agent) {
      agent = parseAgentOption(args.agent)
    }

    return {
      enableAuth: args.enableAuth,
      cwd: args.cwd,
      prompt: args.prompt,
      agent,
    }
  },
})

export function localExampleAgentPath() {
  const require = createRequire(import.meta.url)
  return require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")
}

export async function parseDemoArgs(argv: string[]): Promise<DemoOptions> {
  return run(demoArgsCommand, argv)
}

export function buildDemoRunConfig(options: DemoOptions): {
  agent: string | RegistryAgent["distribution"]
  cwd: string
  prompt: string
} {
  if (options.agent) {
    return {
      agent: options.agent,
      cwd: options.cwd,
      prompt: options.prompt,
    }
  }

  if (options.enableAuth) {
    return {
      agent: "claude-code",
      cwd: options.cwd,
      prompt: options.prompt,
    }
  }

  return {
    agent: {
      type: "binary",
      cmd: "node",
      args: [localExampleAgentPath()],
    },
    cwd: options.cwd,
    prompt: options.prompt,
  }
}

async function runDemo(options: DemoOptions) {
  const { runAgent } = await import("../client.js")
  const config = buildDemoRunConfig(options)

  const session = await runAgent(
    {
      agent: config.agent,
      cwd: config.cwd,
      mcpServers: [],
      systemPrompt: "You are a demo session. Respond helpfully and concisely.",
    },
    {
      async requestPermission(params: any) {
        const allowOption = params.options?.find((option: any) => option.kind === "allow_once")
        if (!allowOption) {
          return { outcome: { outcome: "cancelled" } }
        }
        return {
          outcome: {
            outcome: "selected",
            optionId: allowOption.optionId,
          },
        }
      },
      async sessionUpdate() {
        // no-op for demo
      },
    },
  )

  try {
    const result = await session.prompt(config.prompt)
    process.stdout.write(JSON.stringify(result, null, 2) + "\n")

    const history = await session.getHistory()
    process.stdout.write(`history messages: ${history.length}\n`)
  } finally {
    await session.stop()
  }
}

export async function main(argv: string[]) {
  const options = await parseDemoArgs(argv)
  await runDemo(options)
}

if (import.meta.main) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error)
    process.exit(1)
  })
}
