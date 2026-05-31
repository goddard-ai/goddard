import { constants as fsConstants } from "node:fs"
import { access } from "node:fs/promises"
import { delimiter, join } from "node:path"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import type { AgentsConfig, StaticSessionParams } from "@goddard-ai/schema/config"

type AgentConfig = {
  agents?: AgentsConfig
  session?: StaticSessionParams
  actions?: {
    session?: StaticSessionParams
  }
  loops?: {
    session?: StaticSessionParams
  }
}

const missingDefaultAgentMessage =
  "No default ACP agent is configured or discoverable. Configure `agents.default`, `session.agent`, or a feature-specific session agent before launching an agent."

/** Resolves the user's preferred default agent from config or a detected local agent executable. */
export async function resolveDefaultAgent(
  config?: AgentConfig,
  feature?: "actions" | "loops",
): Promise<string | AgentDistribution> {
  // 1. Check if the user has explicitly configured a default agent
  if (config) {
    if (feature === "actions" && config.actions?.session?.agent) {
      return config.actions.session.agent
    }
    if (feature === "loops" && config.loops?.session?.agent) {
      return config.loops.session.agent
    }
    if (config.session?.agent) {
      return config.session.agent
    }
    if (config.agents?.default) {
      return config.agents.default
    }
  }

  // 2. Inspect the environment for supported executables
  const possibleAgents = ["codex", "claude", "pi", "gemini"]
  const mappings: Record<string, string> = {
    codex: "codex-acp",
    claude: "claude-acp",
    pi: "pi-acp",
    gemini: "gemini",
  }

  const envPath = process.env.PATH || ""
  const paths = envPath.split(delimiter)
  const isWin = process.platform === "win32"
  const exts = isWin ? (process.env.PATHEXT || ".COM;.EXE;.BAT;.CMD").split(delimiter) : [""]

  for (const agent of possibleAgents) {
    for (const dir of paths) {
      for (const ext of exts) {
        const exe = join(dir, agent + ext)
        try {
          await access(exe, fsConstants.X_OK)
          return mappings[agent]
        } catch {
          // File does not exist or is not executable, continue searching
        }
      }
    }
  }

  throw new Error(missingDefaultAgentMessage)
}
