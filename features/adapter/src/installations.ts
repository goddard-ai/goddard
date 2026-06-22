import {
  getManagedAgentInstallationState,
  getManagedAgentInstallationStates,
  installManagedAgent,
  uninstallManagedAgent,
} from "@goddard-ai/managed-agent/installations"
import type { AdapterCatalogEntry } from "acp-client/node"

import type { AdapterInstallationState } from "./schema.ts"

function toAdapterInstallationState(
  state: Awaited<ReturnType<typeof getManagedAgentInstallationState>>,
) {
  const { managedAgentId, ...rest } = state
  return {
    adapterId: managedAgentId,
    ...rest,
  } satisfies AdapterInstallationState
}

export async function getAdapterInstallationState(
  adapter: AdapterCatalogEntry,
  installedAdapterIds: Set<string>,
) {
  return toAdapterInstallationState(
    await getManagedAgentInstallationState(adapter, installedAdapterIds),
  )
}

export async function getAdapterInstallationStates(adapters: AdapterCatalogEntry[]) {
  return (await getManagedAgentInstallationStates(adapters)).map(toAdapterInstallationState)
}

export async function installAdapter(adapter: AdapterCatalogEntry) {
  return toAdapterInstallationState(await installManagedAgent(adapter))
}

export async function uninstallAdapter(adapterId: string) {
  await uninstallManagedAgent(adapterId)
}
