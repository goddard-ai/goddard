import { constants as fsConstants } from "node:fs"
import { access } from "node:fs/promises"
import { delimiter, join } from "node:path"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import type { UserConfig } from "@goddard-ai/schema/config"

/** Resolves the user's preferred default agent by inspecting configuration and system state. */
export async function resolveDefaultAgent(
  config?: UserConfig,
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

  // 3. Fallback to the first defined mapping as a safe default if no local state provides an answer
  return "pi-acp"
}
