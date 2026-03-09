import { spawn } from "node:child_process"
import { fetchRegistryAgent } from "./registry.js"

export async function spawnAgentServer(agentName: string) {
  const agent = await fetchRegistryAgent(agentName)
  if (!agent) {
    throw new Error(`Agent not found: ${agentName}`)
  }

  let cmd: string
  let args: string[]

  if (agent.distribution.type === "npx" && agent.distribution.package) {
    cmd = "npx"
    args = ["-y", agent.distribution.package]
  } else if (agent.distribution.type === "binary" && agent.distribution.cmd) {
    cmd = agent.distribution.cmd
    args = agent.distribution.args || []
  } else if (agent.distribution.type === "uvx" && agent.distribution.package) {
    cmd = "uvx"
    args = [agent.distribution.package]
  } else {
    throw new Error("Unsupported agent distribution")
  }

  return spawn(cmd, args)
}
