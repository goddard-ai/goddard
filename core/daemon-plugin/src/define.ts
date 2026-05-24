import type { HttpRouteTree as BackendRouteTree } from "@goddard-ai/backend-plugin"
import type { HttpRouteTree as IpcRouteTree } from "@goddard-ai/ipc"

import type {
  ConfigDefinition,
  DbSchemaDefinition,
  FeatureExtensions,
  Plugin,
  RouteHandlers,
  SetupContext,
} from "./contracts.ts"

type RegisterFunction = (...args: never[]) => void | Promise<void>

type ConsumedPlugins<TConsumes> =
  Extract<TConsumes, readonly Plugin[]> extends never ? [] : Extract<TConsumes, readonly Plugin[]>

type OptionalPluginField<TKey extends string, TValue> = undefined extends TValue
  ? {}
  : { readonly [K in TKey]: TValue }

type PluginShape<
  TName extends string,
  TConsumes extends readonly Plugin[] | undefined,
  TConfig extends ConfigDefinition | undefined,
  TDb extends DbSchemaDefinition | undefined,
  TBackendRoutes extends BackendRouteTree | undefined,
  TIpcRoutes extends IpcRouteTree | undefined,
  TLifecycle,
  TRegister extends RegisterFunction | undefined,
> = { readonly name: TName } & OptionalPluginField<"consumes", TConsumes> &
  OptionalPluginField<"config", TConfig> &
  OptionalPluginField<"db", TDb> &
  OptionalPluginField<"backendRoutes", TBackendRoutes> &
  OptionalPluginField<"ipcRoutes", TIpcRoutes> &
  OptionalPluginField<"lifecycle", TLifecycle> &
  OptionalPluginField<"register", TRegister>

type PluginOptions<
  TName extends string,
  TConsumes extends readonly Plugin[] | undefined,
  TConfig extends ConfigDefinition | undefined,
  TDb extends DbSchemaDefinition | undefined,
  TBackendRoutes extends BackendRouteTree | undefined,
  TIpcRoutes extends IpcRouteTree | undefined,
  TLifecycle,
  TRegister extends RegisterFunction | undefined,
> = {
  readonly name: TName
  readonly consumes?: TConsumes
  readonly provides?: never
  readonly config?: TConfig
  readonly db?: TDb
  readonly backendRoutes?: TBackendRoutes
  readonly ipcRoutes?: TIpcRoutes
  readonly lifecycle?: TLifecycle
  readonly register?: TRegister
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

type RouteHandlerContributions<TIpcRoutes> = TIpcRoutes extends IpcRouteTree
  ? {
      readonly routeHandlers: RouteHandlers<TIpcRoutes>
    }
  : {
      readonly routeHandlers?: never
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
> = RouteHandlerContributions<TIpcRoutes> & ProvidesContribution<TProvides>

type DefinePlugin = {
  <
    const TName extends string,
    const TConsumes extends readonly Plugin[] | undefined,
    const TConfig extends ConfigDefinition | undefined,
    const TDb extends DbSchemaDefinition | undefined,
    const TBackendRoutes extends BackendRouteTree | undefined,
    const TIpcRoutes extends IpcRouteTree,
    const TLifecycle,
    const TRegister extends RegisterFunction | undefined,
    const TProvides extends FeatureExtensions | undefined,
  >(
    plugin: PluginOptions<
      TName,
      TConsumes,
      TConfig,
      TDb,
      TBackendRoutes,
      TIpcRoutes,
      TLifecycle,
      TRegister
    > & {
      readonly ipcRoutes: TIpcRoutes
      readonly setup: RequiredPluginSetup<
        TConsumes,
        PluginShape<
          TName,
          TConsumes,
          TConfig,
          TDb,
          TBackendRoutes,
          TIpcRoutes,
          TLifecycle,
          TRegister
        >,
        TIpcRoutes,
        TProvides
      >
    },
  ): PluginShape<
    TName,
    TConsumes,
    TConfig,
    TDb,
    TBackendRoutes,
    TIpcRoutes,
    TLifecycle,
    TRegister
  > & {
    readonly setup: RequiredPluginSetup<
      TConsumes,
      PluginShape<
        TName,
        TConsumes,
        TConfig,
        TDb,
        TBackendRoutes,
        TIpcRoutes,
        TLifecycle,
        TRegister
      >,
      TIpcRoutes,
      TProvides
    >
  }
  <
    const TName extends string,
    const TConsumes extends readonly Plugin[] | undefined,
    const TConfig extends ConfigDefinition | undefined,
    const TDb extends DbSchemaDefinition | undefined,
    const TBackendRoutes extends BackendRouteTree | undefined,
    const TIpcRoutes extends IpcRouteTree | undefined,
    const TLifecycle,
    const TRegister extends RegisterFunction | undefined,
    const TProvides extends FeatureExtensions | undefined,
  >(
    plugin: PluginOptions<
      TName,
      TConsumes,
      TConfig,
      TDb,
      TBackendRoutes,
      TIpcRoutes,
      TLifecycle,
      TRegister
    > & {
      readonly setup?: PluginSetup<
        TConsumes,
        PluginShape<
          TName,
          TConsumes,
          TConfig,
          TDb,
          TBackendRoutes,
          TIpcRoutes,
          TLifecycle,
          TRegister
        >,
        undefined,
        TProvides
      >
    },
  ): PluginShape<
    TName,
    TConsumes,
    TConfig,
    TDb,
    TBackendRoutes,
    TIpcRoutes,
    TLifecycle,
    TRegister
  > & {
    readonly setup?: PluginSetup<
      TConsumes,
      PluginShape<
        TName,
        TConsumes,
        TConfig,
        TDb,
        TBackendRoutes,
        TIpcRoutes,
        TLifecycle,
        TRegister
      >,
      undefined,
      TProvides
    >
  }
}

/** Preserves the exact daemon plugin object for composition-time type inference. */
export const definePlugin = ((plugin: Plugin) => plugin) as DefinePlugin
