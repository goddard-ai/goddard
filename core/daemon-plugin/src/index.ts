/** Internal daemon plugin support contracts for statically composed feature packages. */
import type { IpcSchema } from "@goddard-ai/ipc"

/** Daemon plugin shape used to constrain feature plugin values without widening them. */
export type DaemonPluginDefinition = {
  readonly name: string
  readonly ipc?: IpcSchema
  readonly lifecycle?: unknown
  readonly register?: (...args: never[]) => void | Promise<void>
}

/** Preserves the exact daemon plugin object for composition-time type inference. */
export function defineDaemonPlugin<const TPlugin extends DaemonPluginDefinition>(plugin: TPlugin) {
  return plugin
}
