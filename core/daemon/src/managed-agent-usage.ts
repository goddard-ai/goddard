export type ManagedAgentUsageState = Record<
  string,
  {
    readonly lastUsedAt: string
  }
>

export type ManagedAgentUsageStore = {
  readonly get: () => ManagedAgentUsageState | undefined
  readonly set: (state: ManagedAgentUsageState) => void
}

const MANAGED_AGENT_PROACTIVE_UPDATE_ACTIVE_WINDOW_MS = 30 * 24 * 60 * 60 * 1000

export function recordManagedAgentUsed(
  usageStore: ManagedAgentUsageStore,
  agentId: string,
  lastUsedAt: string,
) {
  usageStore.set({
    ...usageStore.get(),
    [agentId]: { lastUsedAt },
  })
}

export function isManagedAgentActiveForProactiveUpdate(
  usageState: ManagedAgentUsageState,
  agentId: string,
  nowMs: number,
) {
  const lastUsedAtMs = Date.parse(usageState[agentId]?.lastUsedAt ?? "")
  if (!Number.isFinite(lastUsedAtMs)) {
    return false
  }

  const ageMs = nowMs - lastUsedAtMs
  return ageMs >= 0 && ageMs <= MANAGED_AGENT_PROACTIVE_UPDATE_ACTIVE_WINDOW_MS
}
