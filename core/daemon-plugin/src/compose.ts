import {
  composeBackendRoutes,
  type HttpRouteTree as BackendRouteTree,
} from "@goddard-ai/backend-plugin"
import { composeIpcRoutes, type HttpRouteTree as IpcRouteTree } from "@goddard-ai/ipc"
import type { SchemaMigrationPlanner } from "kindstore"

import type {
  Composition,
  ConfigDefinition,
  DaemonLogContextDefinition,
  DbSchemaDefinition,
  EventDefinitions,
  JsonSchemaArtifactDefinition,
  Plugin,
} from "./contracts.ts"
import type { UnionToIntersection } from "./type-utils.ts"

type PluginDb<TPlugin> = TPlugin extends {
  readonly db: { readonly schema: infer TDb extends DbSchemaDefinition }
}
  ? TDb
  : {}
type ComposedPluginDb<TPlugins extends readonly Plugin[]> = UnionToIntersection<
  PluginDb<TPlugins[number]>
> &
  DbSchemaDefinition

/** Composes statically imported daemon feature plugins and validates dependency ownership. */
export function composePlugins<const TPlugins extends readonly Plugin[]>(plugins: TPlugins) {
  assertUniquePluginNames(plugins)
  assertConsumedPluginsAreComposed(plugins)
  const orderedPlugins = sortPluginsByDependency(plugins)

  const config: Record<string, ConfigDefinition<any, any>> = {}
  const events: EventDefinitions = {}
  const db: DbSchemaDefinition = {}
  const backendRouteTrees: BackendRouteTree[] = []
  const ipcRouteTrees: IpcRouteTree[] = []
  const jsonSchemas: JsonSchemaArtifactDefinition[] = []
  const jsonSchemaNames = new Set<string>()
  const logContexts: DaemonLogContextDefinition[] = []
  const dbMigrations: Array<(planner: SchemaMigrationPlanner<any>) => void> = []

  for (const plugin of orderedPlugins) {
    if (plugin.config) {
      for (const [key, definition] of Object.entries(plugin.config)) {
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
    for (const [name, definition] of Object.entries(plugin.events ?? {})) {
      if (events[name]) {
        throw new Error(`Duplicate daemon plugin event: ${name}`)
      }
      events[name] = definition
    }
    if (plugin.logContext) {
      logContexts.push(plugin.logContext)
    }
    if (plugin.db?.migrate) {
      dbMigrations.push(plugin.db.migrate)
    }
    for (const [key, kind] of Object.entries(plugin.db?.schema ?? {})) {
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
    events,
    db: {
      schema: db as ComposedPluginDb<TPlugins>,
      migrate:
        dbMigrations.length > 0
          ? (planner) => {
              for (const migrate of dbMigrations) {
                migrate(planner)
              }
            }
          : undefined,
    },
    logContexts,
  } satisfies Composition
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
