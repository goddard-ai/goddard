/** Internal app plugin support contracts for statically composed feature packages. */

/** App plugin shape used to constrain feature plugin values without widening them. */
export type AppPluginDefinition = {
  readonly name: string
  readonly sdk?: unknown
  readonly routes?: unknown
  readonly commands?: unknown
  readonly register?: (...args: never[]) => void | Promise<void>
}

/** Preserves the exact app plugin object for composition-time type inference. */
export function defineAppPlugin<const TPlugin extends AppPluginDefinition>(plugin: TPlugin) {
  return plugin
}
