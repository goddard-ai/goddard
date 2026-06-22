import {
  ListManagedAgentsRequest,
  ManagedAgentCatalogEntry,
  ManagedAgentInstall,
  ManagedAgentInstallAgent,
  ManagedAgentInstallState,
  type ListManagedAgentsRequest as ListManagedAgentsRequestType,
  type ListManagedAgentsResponse,
  type ManagedAgentCatalogEntry as ManagedAgentCatalogEntryType,
  type ManagedAgentInstallAgent as ManagedAgentInstallAgentType,
  type ManagedAgentInstallationState,
  type ManagedAgentInstallState as ManagedAgentInstallStateType,
  type ManagedAgentInstall as ManagedAgentInstallType,
} from "@goddard-ai/managed-agent/schema"
import { z } from "zod"

export const ListAdaptersRequest = ListManagedAgentsRequest
export type ListAdaptersRequest = ListManagedAgentsRequestType
export type ListAdaptersRequestType = ListAdaptersRequest

export const AdapterManagedInstallAgent = ManagedAgentInstallAgent
export type AdapterManagedInstallAgent = ManagedAgentInstallAgentType

export const AdapterManagedInstallState = ManagedAgentInstallState
export type AdapterManagedInstallState = ManagedAgentInstallStateType

export const AdapterManagedInstall = ManagedAgentInstall
export type AdapterManagedInstall = ManagedAgentInstallType

export const AdapterCatalogEntry = ManagedAgentCatalogEntry
export type AdapterCatalogEntry = ManagedAgentCatalogEntryType

export type AdapterInstallationState = Omit<ManagedAgentInstallationState, "managedAgentId"> & {
  adapterId: string
}

export type ListAdaptersResponse = Omit<
  ListManagedAgentsResponse,
  "defaultManagedAgentId" | "installations" | "managedAgents"
> & {
  adapters: AdapterCatalogEntry[]
  installations: AdapterInstallationState[]
  defaultAdapterId: string | null
}

export const InstallAdapterRequest = z.strictObject({
  adapterId: z.string().min(1),
})

export type InstallAdapterRequest = z.infer<typeof InstallAdapterRequest>

export type InstallAdapterResponse = {
  adapter: AdapterCatalogEntry
  installation: AdapterInstallationState
}

export const UninstallAdapterRequest = z.strictObject({
  adapterId: z.string().min(1),
})

export type UninstallAdapterRequest = z.infer<typeof UninstallAdapterRequest>

export type UninstallAdapterResponse = {
  adapterId: string
}
