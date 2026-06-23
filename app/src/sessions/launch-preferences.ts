import type { ListAgentsResponse } from "@goddard-ai/sdk"
import { signal } from "@preact/signals"

export const preferredLaunchAgentId = signal<string | null>(null)
export const preferredLaunchCwdByProject = signal<Record<string, string | null>>({})

export function recordSessionLaunchAgentUse(agentId: string | null | undefined) {
  if (agentId) {
    preferredLaunchAgentId.value = agentId
  }
}

export function recordSessionLaunchCwdUse(
  projectPath: string | null | undefined,
  cwd: string | null | undefined,
) {
  if (!projectPath) {
    return
  }

  preferredLaunchCwdByProject.value = {
    ...preferredLaunchCwdByProject.value,
    [projectPath]: cwd && cwd !== projectPath ? cwd : null,
  }
}

export function resolvePreferredLaunchCwd(
  projectPath: string,
  subpackages: readonly { path: string }[],
) {
  const preferredCwd = preferredLaunchCwdByProject.value[projectPath]

  if (preferredCwd && subpackages.some((subpackage) => subpackage.path === preferredCwd)) {
    return preferredCwd
  }

  return projectPath
}

export function resolvePreferredLaunchAgentId(managedAgentCatalog: ListAgentsResponse) {
  const availableAgentIds = new Set(
    managedAgentCatalog.agents.map((managedAgent) => managedAgent.id),
  )

  if (preferredLaunchAgentId.value && availableAgentIds.has(preferredLaunchAgentId.value)) {
    return preferredLaunchAgentId.value
  }

  if (
    managedAgentCatalog.defaultAgentId &&
    availableAgentIds.has(managedAgentCatalog.defaultAgentId)
  ) {
    return managedAgentCatalog.defaultAgentId
  }

  return managedAgentCatalog.agents[0]?.id ?? null
}
