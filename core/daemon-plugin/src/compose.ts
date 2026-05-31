import {
  composeBackendRoutes,
  type HttpRouteTree as BackendRouteTree,
} from "@goddard-ai/backend-plugin"
import { composeIpcRoutes, type HttpRouteTree as IpcRouteTree } from "@goddard-ai/ipc"

import type {
  Composition,
  ConfigDefinition,
  DbSchemaDefinition,
  JsonSchemaArtifactDefinition,
  Plugin,
} from "./contracts.ts"

/** Composes statically imported daemon feature plugins and validates dependency ownership. */
export function composePlugins(plugins: readonly Plugin[]) {
  assertUniquePluginNames(plugins)
  assertConsumedPluginsAreComposed(plugins)
  const orderedPlugins = sortPluginsByDependency(plugins)

  const config: Record<string, ConfigDefinition<any, any>> = {}
  const db: DbSchemaDefinition = {}
  const backendRouteTrees: BackendRouteTree[] = []
  const ipcRouteTrees: IpcRouteTree[] = []
  const jsonSchemas: JsonSchemaArtifactDefinition[] = []
  const jsonSchemaNames = new Set<string>()

  for (const plugin of orderedPlugins) {
    if (plugin.config) {
      for (const [key, definition] of getPluginConfigEntries(plugin)) {
        if (config[key]) {
          throw new Error(`Duplicate daemon plugin config namespace: ${key}`)
        }
        config[key] = definition
      }
    }
    if (plugin.ipcRoutes) {
      ipcRouteTrees.push(plugin.ipcRoutes)
    }
    for (const schema of plugin.jsonSchemas ?? []) {
      if (jsonSchemaNames.has(schema.name)) {
        throw new Error(`Duplicate daemon plugin JSON schema artifact: ${schema.name}`)
      }
      jsonSchemaNames.add(schema.name)
      jsonSchemas.push(schema)
    }
    if (plugin.backendRoutes) {
      backendRouteTrees.push(plugin.backendRoutes)
    }
    for (const [key, kind] of Object.entries(plugin.db ?? {})) {
      if (db[key]) {
        throw new Error(`Duplicate daemon plugin DB collection: ${key}`)
      }
      db[key] = kind
    }
  }

  return {
    plugins: orderedPlugins,
    ipcRoutes: composeIpcRoutes(ipcRouteTrees),
    backendRoutes: composeBackendRoutes(backendRouteTrees),
    config,
    jsonSchemas,
    db,
  } satisfies Composition
}

function getPluginConfigEntries(plugin: Plugin): Array<[string, ConfigDefinition<any, any>]> {
  if (!plugin.config) {
    return []
  }

  if ("schema" in plugin.config) {
    return [[plugin.name, plugin.config as ConfigDefinition<any, any>]]
  }

  return Object.entries(plugin.config) as Array<[string, ConfigDefinition<any, any>]>
}

function assertUniquePluginNames(plugins: readonly Plugin[]) {
  const names = new Set<string>()

  for (const plugin of plugins) {
    if (names.has(plugin.name)) {
      throw new Error(`Duplicate daemon plugin: ${plugin.name}`)
    }
    names.add(plugin.name)
  }
}

function assertConsumedPluginsAreComposed(plugins: readonly Plugin[]) {
  const pluginNames = new Set(plugins.map((plugin) => plugin.name))

  for (const plugin of plugins) {
    for (const consumedPlugin of plugin.consumes ?? []) {
      if (!pluginNames.has(consumedPlugin.name)) {
        throw new Error(
          `Daemon plugin ${plugin.name} consumes ${consumedPlugin.name}, but ${consumedPlugin.name} is not composed.`,
        )
      }
    }
  }
}

function sortPluginsByDependency(plugins: readonly Plugin[]) {
  const orderedPlugins: Plugin[] = []
  const visiting = new Set<string>()
  const visited = new Set<string>()
  const pluginsByName = new Map(plugins.map((plugin) => [plugin.name, plugin]))

  function visit(plugin: Plugin, path: readonly string[]) {
    if (visited.has(plugin.name)) {
      return
    }

    if (visiting.has(plugin.name)) {
      throw new Error(`Circular daemon plugin dependency: ${[...path, plugin.name].join(" -> ")}`)
    }

    visiting.add(plugin.name)

    for (const consumedPlugin of plugin.consumes ?? []) {
      visit(pluginsByName.get(consumedPlugin.name) ?? consumedPlugin, [...path, plugin.name])
    }

    visiting.delete(plugin.name)
    visited.add(plugin.name)
    orderedPlugins.push(plugin)
  }

  for (const plugin of plugins) {
    visit(plugin, [])
  }

  return orderedPlugins
}
