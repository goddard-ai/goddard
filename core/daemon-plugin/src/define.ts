import type { HttpRouteTree as BackendRouteTree } from "@goddard-ai/backend-plugin"
import type { HttpRouteTree as IpcRouteTree } from "@goddard-ai/ipc"

import type {
  BackendEventHandler,
  ConfigDefinitions,
  DaemonLogContextDefinition,
  DbDefinition,
  EventDefinitions,
  FeatureExtensions,
  IpcHandlers,
  JsonSchemaArtifactDefinition,
  Plugin,
  SetupContext,
} from "./contracts.ts"

type ConsumedPlugins<TConsumes> =
  Extract<TConsumes, readonly Plugin[]> extends never ? [] : Extract<TConsumes, readonly Plugin[]>

type OptionalPluginField<TKey extends string, TValue> = undefined extends TValue
  ? {}
  : { readonly [K in TKey]: TValue }

type PluginShape<
  TName extends string,
  TConsumes extends readonly Plugin[] | undefined,
  TConfig extends ConfigDefinitions | undefined,
  TJsonSchemas extends readonly JsonSchemaArtifactDefinition[] | undefined,
  TEvents extends EventDefinitions | undefined,
  TDb extends DbDefinition | undefined,
  TBackendRoutes extends BackendRouteTree | undefined,
  TIpcRoutes extends IpcRouteTree | undefined,
> = { readonly name: TName } & OptionalPluginField<"consumes", TConsumes> &
  OptionalPluginField<"config", TConfig> &
  OptionalPluginField<"jsonSchemas", TJsonSchemas> &
  OptionalPluginField<"events", TEvents> &
  OptionalPluginField<"db", TDb> &
  OptionalPluginField<"backendRoutes", TBackendRoutes> &
  OptionalPluginField<"ipcRoutes", TIpcRoutes> &
  OptionalPluginField<"logContext", DaemonLogContextDefinition | undefined>

type PublicPluginShape<
  TName extends string,
  TConsumes extends readonly Plugin[] | undefined,
  TConfig extends ConfigDefinitions | undefined,
  TJsonSchemas extends readonly JsonSchemaArtifactDefinition[] | undefined,
  TEvents extends EventDefinitions | undefined,
  TDb extends DbDefinition | undefined,
  TBackendRoutes extends BackendRouteTree | undefined,
  TIpcRoutes extends IpcRouteTree | undefined,
> = PluginShape<
  TName,
  undefined extends TConsumes ? undefined : readonly Plugin[],
  TConfig,
  TJsonSchemas,
  TEvents,
  TDb,
  TBackendRoutes,
  TIpcRoutes
>

type PluginOptions<
  TName extends string,
  TConsumes extends readonly Plugin[] | undefined,
  TConfig extends ConfigDefinitions | undefined,
  TJsonSchemas extends readonly JsonSchemaArtifactDefinition[] | undefined,
  TEvents extends EventDefinitions | undefined,
  TDb extends DbDefinition | undefined,
  TBackendRoutes extends BackendRouteTree | undefined,
  TIpcRoutes extends IpcRouteTree | undefined,
> = {
  readonly name: TName
  readonly consumes?: TConsumes
  readonly provides?: never
  readonly config?: TConfig
  readonly jsonSchemas?: TJsonSchemas
  readonly events?: TEvents
  readonly db?: TDb
  readonly backendRoutes?: TBackendRoutes
  readonly ipcRoutes?: TIpcRoutes
  readonly logContext?: DaemonLogContextDefinition
}

type PluginSetup<
  TConsumes extends readonly Plugin[] | undefined,
  TSelf,
  TIpcRoutes,
  TProvides extends FeatureExtensions | undefined,
> = (
  context: SetupContext<ConsumedPlugins<TConsumes>, TSelf>,
) =>
  | void
  | SetupContributions<TIpcRoutes, TProvides>
  | Promise<void | SetupContributions<TIpcRoutes, TProvides>>

type RequiredPluginSetup<
  TConsumes extends readonly Plugin[] | undefined,
  TSelf,
  TIpcRoutes,
  TProvides extends FeatureExtensions | undefined,
> = (
  context: SetupContext<ConsumedPlugins<TConsumes>, TSelf>,
) => SetupContributions<TIpcRoutes, TProvides> | Promise<SetupContributions<TIpcRoutes, TProvides>>

type PublicPluginSetup<TIpcRoutes, TProvides extends FeatureExtensions | undefined> = (
  context: any,
) =>
  | void
  | SetupContributions<TIpcRoutes, TProvides>
  | Promise<void | SetupContributions<TIpcRoutes, TProvides>>

type RequiredPublicPluginSetup<TIpcRoutes, TProvides extends FeatureExtensions | undefined> = (
  context: any,
) => SetupContributions<TIpcRoutes, TProvides> | Promise<SetupContributions<TIpcRoutes, TProvides>>

type IpcHandlerContributions<TIpcRoutes> = TIpcRoutes extends IpcRouteTree
  ? {
      readonly ipcHandlers: IpcHandlers<TIpcRoutes>
    }
  : {
      readonly ipcHandlers?: never
    }

type ProvidesContribution<TProvides> = TProvides extends FeatureExtensions
  ? {
      readonly provides: TProvides
    }
  : {
      readonly provides?: never
    }

type SetupContributions<
  TIpcRoutes,
  TProvides extends FeatureExtensions | undefined,
> = IpcHandlerContributions<TIpcRoutes> &
  ProvidesContribution<TProvides> & {
    readonly backendEventHandlers?: readonly BackendEventHandler[]
    readonly close?: () => void | Promise<void>
  }

type DefinePlugin = {
  <
    const TName extends string,
    const TConsumes extends readonly Plugin[] | undefined,
    const TConfig extends ConfigDefinitions | undefined,
    const TJsonSchemas extends readonly JsonSchemaArtifactDefinition[] | undefined,
    const TEvents extends EventDefinitions | undefined,
    const TDb extends DbDefinition | undefined,
    const TBackendRoutes extends BackendRouteTree | undefined,
    const TIpcRoutes extends IpcRouteTree,
    const TProvides extends FeatureExtensions | undefined,
  >(
    plugin: PluginOptions<
      TName,
      TConsumes,
      TConfig,
      TJsonSchemas,
      TEvents,
      TDb,
      TBackendRoutes,
      TIpcRoutes
    > & {
      readonly ipcRoutes: TIpcRoutes
      readonly setup: RequiredPluginSetup<
        TConsumes,
        PluginShape<
          TName,
          TConsumes,
          TConfig,
          TJsonSchemas,
          TEvents,
          TDb,
          TBackendRoutes,
          TIpcRoutes
        >,
        TIpcRoutes,
        TProvides
      >
    },
  ): PublicPluginShape<
    TName,
    TConsumes,
    TConfig,
    TJsonSchemas,
    TEvents,
    TDb,
    TBackendRoutes,
    TIpcRoutes
  > & {
    readonly setup: RequiredPublicPluginSetup<TIpcRoutes, TProvides>
  }
  <
    const TName extends string,
    const TConsumes extends readonly Plugin[] | undefined,
    const TConfig extends ConfigDefinitions | undefined,
    const TJsonSchemas extends readonly JsonSchemaArtifactDefinition[] | undefined,
    const TEvents extends EventDefinitions | undefined,
    const TDb extends DbDefinition | undefined,
    const TBackendRoutes extends BackendRouteTree | undefined,
    const TIpcRoutes extends IpcRouteTree | undefined,
    const TProvides extends FeatureExtensions | undefined,
  >(
    plugin: PluginOptions<
      TName,
      TConsumes,
      TConfig,
      TJsonSchemas,
      TEvents,
      TDb,
      TBackendRoutes,
      TIpcRoutes
    > & {
      readonly setup?: PluginSetup<
        TConsumes,
        PluginShape<
          TName,
          TConsumes,
          TConfig,
          TJsonSchemas,
          TEvents,
          TDb,
          TBackendRoutes,
          TIpcRoutes
        >,
        undefined,
        TProvides
      >
    },
  ): PublicPluginShape<
    TName,
    TConsumes,
    TConfig,
    TJsonSchemas,
    TEvents,
    TDb,
    TBackendRoutes,
    TIpcRoutes
  > & {
    readonly setup?: PublicPluginSetup<undefined, TProvides>
  }
}

/** Preserves the exact daemon plugin object for composition-time type inference. */
export const definePlugin = ((plugin: Plugin) => plugin) as DefinePlugin
