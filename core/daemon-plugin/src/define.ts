import type { IpcSchema } from "@goddard-ai/ipc"

import type {
  ConfigDefinition,
  DbSchemaDefinition,
  FeatureExtensions,
  Plugin,
  RequestHandlers,
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
  TIpc extends IpcSchema | undefined,
  TLifecycle,
  TRegister extends RegisterFunction | undefined,
> = { readonly name: TName } & OptionalPluginField<"consumes", TConsumes> &
  OptionalPluginField<"config", TConfig> &
  OptionalPluginField<"db", TDb> &
  OptionalPluginField<"ipc", TIpc> &
  OptionalPluginField<"lifecycle", TLifecycle> &
  OptionalPluginField<"register", TRegister>

type PluginOptions<
  TName extends string,
  TConsumes extends readonly Plugin[] | undefined,
  TConfig extends ConfigDefinition | undefined,
  TDb extends DbSchemaDefinition | undefined,
  TIpc extends IpcSchema | undefined,
  TLifecycle,
  TRegister extends RegisterFunction | undefined,
> = {
  readonly name: TName
  readonly consumes?: TConsumes
  readonly provides?: never
  readonly config?: TConfig
  readonly db?: TDb
  readonly ipc?: TIpc
  readonly lifecycle?: TLifecycle
  readonly register?: TRegister
}

type PluginSetup<
  TConsumes extends readonly Plugin[] | undefined,
  TSelf,
  TIpc,
  TProvides extends FeatureExtensions | undefined,
> = (
  context: SetupContext<ConsumedPlugins<TConsumes>, TSelf>,
) =>
  | void
  | SetupContributions<TIpc, TProvides>
  | Promise<void | SetupContributions<TIpc, TProvides>>

type RequiredPluginSetup<
  TConsumes extends readonly Plugin[] | undefined,
  TSelf,
  TIpc,
  TProvides extends FeatureExtensions | undefined,
> = (
  context: SetupContext<ConsumedPlugins<TConsumes>, TSelf>,
) => SetupContributions<TIpc, TProvides> | Promise<SetupContributions<TIpc, TProvides>>

type RequestHandlerContributions<TIpc> = TIpc extends IpcSchema
  ? {
      readonly requestHandlers: RequestHandlers<TIpc>
    }
  : {
      readonly requestHandlers?: never
    }

type ProvidesContribution<TProvides> = TProvides extends FeatureExtensions
  ? {
      readonly provides: TProvides
    }
  : {
      readonly provides?: never
    }

type SetupContributions<
  TIpc,
  TProvides extends FeatureExtensions | undefined,
> = RequestHandlerContributions<TIpc> & ProvidesContribution<TProvides>

type DefinePlugin = {
  <
    const TName extends string,
    const TConsumes extends readonly Plugin[] | undefined,
    const TConfig extends ConfigDefinition | undefined,
    const TDb extends DbSchemaDefinition | undefined,
    const TIpc extends IpcSchema,
    const TLifecycle,
    const TRegister extends RegisterFunction | undefined,
    const TProvides extends FeatureExtensions | undefined,
  >(
    plugin: PluginOptions<TName, TConsumes, TConfig, TDb, TIpc, TLifecycle, TRegister> & {
      readonly ipc: TIpc
      readonly setup: RequiredPluginSetup<
        TConsumes,
        PluginShape<TName, TConsumes, TConfig, TDb, TIpc, TLifecycle, TRegister>,
        TIpc,
        TProvides
      >
    },
  ): PluginShape<TName, TConsumes, TConfig, TDb, TIpc, TLifecycle, TRegister> & {
    readonly setup: RequiredPluginSetup<
      TConsumes,
      PluginShape<TName, TConsumes, TConfig, TDb, TIpc, TLifecycle, TRegister>,
      TIpc,
      TProvides
    >
  }
  <
    const TName extends string,
    const TConsumes extends readonly Plugin[] | undefined,
    const TConfig extends ConfigDefinition | undefined,
    const TDb extends DbSchemaDefinition | undefined,
    const TLifecycle,
    const TRegister extends RegisterFunction | undefined,
    const TProvides extends FeatureExtensions | undefined,
  >(
    plugin: PluginOptions<TName, TConsumes, TConfig, TDb, undefined, TLifecycle, TRegister> & {
      readonly ipc?: undefined
      readonly setup?: PluginSetup<
        TConsumes,
        PluginShape<TName, TConsumes, TConfig, TDb, undefined, TLifecycle, TRegister>,
        undefined,
        TProvides
      >
    },
  ): PluginShape<TName, TConsumes, TConfig, TDb, undefined, TLifecycle, TRegister> & {
    readonly setup?: PluginSetup<
      TConsumes,
      PluginShape<TName, TConsumes, TConfig, TDb, undefined, TLifecycle, TRegister>,
      undefined,
      TProvides
    >
  }
}

/** Preserves the exact daemon plugin object for composition-time type inference. */
export const definePlugin = ((plugin: Plugin) => plugin) as DefinePlugin
