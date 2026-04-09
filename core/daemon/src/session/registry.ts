import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"

/** Fetches one ACP agent registry entry by its registry id. */
export async function fetchRegistryAgent(agentName: string): Promise<AgentDistribution | null> {
  try {
    const url = `https://raw.githubusercontent.com/agentclientprotocol/registry/refs/heads/main/${agentName}/agent.json`
    const res = await fetch(url)
    if (!res.ok) {
      return null
    }
    return (await res.json()) as AgentDistribution
  } catch {
    return null
  }
}
