/** Internal SDK plugin support contracts for statically composed feature packages. */

/** SDK plugin shape used to constrain feature plugin values without widening them. */
export type SdkPluginDefinition = {
  readonly name: string
  readonly namespace: string
  readonly create: (...args: any[]) => object
}

/** Preserves the exact SDK plugin object for composition-time type inference. */
export function defineSdkPlugin<const TPlugin extends SdkPluginDefinition>(plugin: TPlugin) {
  return plugin
}
