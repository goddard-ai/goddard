import type { AgentDistribution } from "@goddard-ai/schema/session-server"

export interface RegistryAgent {
  name: string
  version: string
  description?: string
  author?: string
  distribution: AgentDistribution
}

export async function fetchRegistryAgent(agentName: string): Promise<RegistryAgent | null> {
  try {
    const url = `https://raw.githubusercontent.com/agentclientprotocol/registry/main/agents/${agentName}/agent.json`
    const res = await fetch(url)
    if (!res.ok) {
      return null
    }
    return (await res.json()) as RegistryAgent
  } catch {
    return null
  }
}
