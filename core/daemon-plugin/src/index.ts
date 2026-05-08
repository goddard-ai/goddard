/** Internal daemon plugin support contracts for statically composed feature packages. */
import type { IpcSchema } from "@goddard-ai/ipc"

/** Named feature extensions exposed by one daemon plugin to plugins that consume it. */
export type DaemonFeatureExtensions = Record<string, unknown>

type EmptyDaemonFeatureExtensions = Record<never, never>

type UnionToIntersection<TUnion> = (
  TUnion extends unknown ? (value: TUnion) => void : never
) extends (value: infer TIntersection) => void
  ? TIntersection
  : never

/** Extracts the first-class context fields provided by one daemon plugin. */
export type InferDaemonPluginProvides<TPlugin> = TPlugin extends {
  readonly provides: infer TProvides extends DaemonFeatureExtensions
}
  ? TProvides
  : EmptyDaemonFeatureExtensions

/** Infers setup context fields from the plugins listed in one `consumes` declaration. */
export type DaemonPluginSetupContext<TConsumes extends readonly unknown[]> = UnionToIntersection<
  InferDaemonPluginProvides<TConsumes[number]>
>

/** Daemon plugin shape used to constrain feature plugin values without widening them. */
export type DaemonPluginDefinition = {
  readonly name: string
  readonly consumes?: readonly DaemonPluginDefinition[]
  readonly provides?: DaemonFeatureExtensions
  readonly ipc?: IpcSchema
  readonly lifecycle?: unknown
  readonly setup?: (
    context: DaemonPluginSetupContext<readonly DaemonPluginDefinition[]>,
  ) => unknown | Promise<unknown>
  readonly register?: (...args: never[]) => void | Promise<void>
}

/** Preserves the exact daemon plugin object for composition-time type inference. */
export function defineDaemonPlugin<
  const TConsumes extends readonly DaemonPluginDefinition[] = [],
  const TPlugin extends Omit<DaemonPluginDefinition, "consumes" | "setup"> = Omit<
    DaemonPluginDefinition,
    "consumes" | "setup"
  >,
>(
  plugin: TPlugin & {
    readonly consumes?: TConsumes
    readonly setup?: (context: DaemonPluginSetupContext<TConsumes>) => unknown | Promise<unknown>
  },
) {
  return plugin
}
