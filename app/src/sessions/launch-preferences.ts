import type { ListAdaptersResponse } from "@goddard-ai/sdk"
import { signal } from "@preact/signals"

const lastUsedLaunchAgentId = signal<string | null>(null)
const preferredLaunchCwdByProject = signal<Record<string, string | null>>({})

export function recordSessionLaunchAgentUse(agentId: string | null | undefined) {
  if (agentId) {
    lastUsedLaunchAgentId.value = agentId
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

export function resolvePreferredLaunchAgentId(adapterCatalog: ListAdaptersResponse) {
  const availableAdapterIds = new Set(adapterCatalog.adapters.map((adapter) => adapter.id))

  if (lastUsedLaunchAgentId.value && availableAdapterIds.has(lastUsedLaunchAgentId.value)) {
    return lastUsedLaunchAgentId.value
  }

  if (adapterCatalog.defaultAdapterId && availableAdapterIds.has(adapterCatalog.defaultAdapterId)) {
    return adapterCatalog.defaultAdapterId
  }

  return adapterCatalog.adapters[0]?.id ?? null
}
