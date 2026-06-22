import type { ListManagedAgentsResponse } from "@goddard-ai/sdk"
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

export function resolvePreferredLaunchAgentId(managedAgentCatalog: ListManagedAgentsResponse) {
  const availableAgentIds = new Set(
    managedAgentCatalog.managedAgents.map((managedAgent) => managedAgent.id),
  )

  if (preferredLaunchAgentId.value && availableAgentIds.has(preferredLaunchAgentId.value)) {
    return preferredLaunchAgentId.value
  }

  if (
    managedAgentCatalog.defaultManagedAgentId &&
    availableAgentIds.has(managedAgentCatalog.defaultManagedAgentId)
  ) {
    return managedAgentCatalog.defaultManagedAgentId
  }

  return managedAgentCatalog.managedAgents[0]?.id ?? null
}
