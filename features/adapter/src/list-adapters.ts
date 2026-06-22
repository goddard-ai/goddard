import {
  installCatalogManagedAgent,
  listManagedAgents,
  uninstallCatalogManagedAgent,
  type ListManagedAgentsContext,
  type ManagedAgentConfigManager,
  type ManagedAgentRegistryService,
} from "@goddard-ai/managed-agent/list-managed-agents"
import type {
  InstallManagedAgentResponse,
  ListManagedAgentsResponse,
  ManagedAgentInstallationState,
} from "@goddard-ai/managed-agent/schema"

import type {
  AdapterInstallationState,
  InstallAdapterRequest,
  InstallAdapterResponse,
  ListAdaptersRequestType,
  ListAdaptersResponse,
  UninstallAdapterRequest,
  UninstallAdapterResponse,
} from "./schema.ts"

export type AdapterRegistryService = ManagedAgentRegistryService
export type AdapterConfigManager = ManagedAgentConfigManager
export type ListAdaptersContext = ListManagedAgentsContext

function toAdapterInstallationState(
  state: ManagedAgentInstallationState,
): AdapterInstallationState {
  const { managedAgentId, ...rest } = state
  return {
    adapterId: managedAgentId,
    ...rest,
  }
}

function toListAdaptersResponse(response: ListManagedAgentsResponse): ListAdaptersResponse {
  const { defaultManagedAgentId, managedAgents, installations, ...rest } = response
  return {
    ...rest,
    adapters: managedAgents,
    installations: installations.map(toAdapterInstallationState),
    defaultAdapterId: defaultManagedAgentId,
  }
}

function toInstallAdapterResponse(response: InstallManagedAgentResponse): InstallAdapterResponse {
  return {
    adapter: response.managedAgent,
    installation: toAdapterInstallationState(response.installation),
  }
}

/** Lists adapters using the managed-agent catalog implementation. */
export async function listAdapters(
  context: ListAdaptersContext,
  request: ListAdaptersRequestType,
): Promise<ListAdaptersResponse> {
  return toListAdaptersResponse(await listManagedAgents(context, request))
}

/** Installs one adapter from the daemon-visible catalog into the local launch set. */
export async function installCatalogAdapter(
  context: ListAdaptersContext,
  { adapterId }: InstallAdapterRequest,
): Promise<InstallAdapterResponse> {
  return toInstallAdapterResponse(
    await installCatalogManagedAgent(context, { managedAgentId: adapterId }),
  )
}

/** Removes one adapter from the local launch set. */
export async function uninstallCatalogAdapter(
  context: ListAdaptersContext,
  { adapterId }: UninstallAdapterRequest,
): Promise<UninstallAdapterResponse> {
  await uninstallCatalogManagedAgent(context, { managedAgentId: adapterId })
  return { adapterId }
}
