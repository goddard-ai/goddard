import type { ListAdaptersResponse } from "@goddard-ai/sdk"

import { SelectorUsageKey, type SelectorUsageStore } from "~/selector-usage.ts"

export function setCurrentSessionLaunchAgent(
  selectorUsage: SelectorUsageStore,
  agentId: string | null | undefined,
) {
  selectorUsage.setCurrentValue(SelectorUsageKey.sessionLaunchAgent, agentId)
}

export function recordSessionLaunchAgentUse(
  selectorUsage: SelectorUsageStore,
  agentId: string | null | undefined,
) {
  selectorUsage.recordUsedValue(SelectorUsageKey.sessionLaunchAgent, agentId)
}

export function setCurrentSessionLaunchCwd(
  selectorUsage: SelectorUsageStore,
  projectPath: string | null | undefined,
  cwd: string | null | undefined,
) {
  if (!projectPath) {
    return
  }

  selectorUsage.setCurrentValue(SelectorUsageKey.sessionLaunchCwd(projectPath), cwd ?? projectPath)
}

export function recordSessionLaunchCwdUse(
  selectorUsage: SelectorUsageStore,
  projectPath: string | null | undefined,
  cwd: string | null | undefined,
) {
  if (!projectPath) {
    return
  }

  selectorUsage.recordUsedValue(SelectorUsageKey.sessionLaunchCwd(projectPath), cwd ?? projectPath)
}

export function recordSessionLaunchUse(
  selectorUsage: SelectorUsageStore,
  input: {
    agentId: string | null | undefined
    branchName: string | null | undefined
    cwd: string | null | undefined
    location: string | null | undefined
    modeValue: string | null | undefined
    modelId: string | null | undefined
    projectPath: string | null | undefined
    repoRoot: string | null | undefined
    thinkingValue: string | null | undefined
  },
) {
  recordSessionLaunchAgentUse(selectorUsage, input.agentId)
  recordSessionLaunchCwdUse(selectorUsage, input.projectPath, input.cwd)
  selectorUsage.recordUsedValue(SelectorUsageKey.sessionLaunchLocation, input.location)

  if (input.repoRoot) {
    selectorUsage.recordUsedValue(
      SelectorUsageKey.sessionLaunchBranch(input.repoRoot),
      input.branchName,
    )
  }

  if (!input.agentId) {
    return
  }

  selectorUsage.recordUsedValue(SelectorUsageKey.sessionControlModel(input.agentId), input.modelId)
  selectorUsage.recordUsedValue(SelectorUsageKey.sessionControlMode(input.agentId), input.modeValue)
  selectorUsage.recordUsedValue(
    SelectorUsageKey.sessionControlThinking(input.agentId),
    input.thinkingValue,
  )
}

export function resolvePreferredLaunchCwd(
  selectorUsage: SelectorUsageStore,
  projectPath: string,
  subpackages: readonly { path: string }[],
) {
  const currentCwd = selectorUsage.getCurrentValue(SelectorUsageKey.sessionLaunchCwd(projectPath))
  const recentCwds = selectorUsage.getRecentUsedValues(
    SelectorUsageKey.sessionLaunchCwd(projectPath),
  )
  const candidates = [currentCwd, ...recentCwds]

  for (const candidate of candidates) {
    if (candidate === projectPath) {
      return projectPath
    }

    if (candidate && subpackages.some((subpackage) => subpackage.path === candidate)) {
      return candidate
    }
  }

  return projectPath
}

export function resolvePreferredLaunchAgentId(
  selectorUsage: SelectorUsageStore,
  adapterCatalog: ListAdaptersResponse,
) {
  const availableAdapterIds = new Set(adapterCatalog.adapters.map((adapter) => adapter.id))
  const candidates = [
    selectorUsage.getCurrentValue(SelectorUsageKey.sessionLaunchAgent),
    ...selectorUsage.getRecentUsedValues(SelectorUsageKey.sessionLaunchAgent),
    adapterCatalog.defaultAdapterId,
  ]

  for (const candidate of candidates) {
    if (candidate && availableAdapterIds.has(candidate)) {
      return candidate
    }
  }

  return adapterCatalog.adapters[0]?.id ?? null
}
