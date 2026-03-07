import type { SessionPlugin, SessionPluginName } from "./types.ts"

const EMBEDDED_PLUGIN_IMPORTS: Record<SessionPluginName, string> = {
  pi: "./pi.ts",
  gemini: "./gemini.ts",
  codex: "./codex.ts",
  pty: "./pty.ts",
}

export type PluginImporter = (specifier: string) => Promise<{ plugin?: SessionPlugin }>

const defaultImporter: PluginImporter = async (specifier) => {
  const module = (await import(specifier)) as { plugin?: SessionPlugin }
  return module
}

export async function loadEmbeddedPlugin(
  name: SessionPluginName,
  importer: PluginImporter = defaultImporter,
): Promise<SessionPlugin> {
  const specifier = EMBEDDED_PLUGIN_IMPORTS[name]
  const loaded = await importer(specifier)

  if (!loaded.plugin) {
    throw new Error(`Plugin module ${specifier} did not export a plugin`)
  }

  return loaded.plugin
}

export function listEmbeddedPlugins(): SessionPluginName[] {
  return Object.keys(EMBEDDED_PLUGIN_IMPORTS) as SessionPluginName[]
}
