/** Internal backend plugin support for feature-owned Rouzer route declarations. */
import type { HttpAction, HttpNode, HttpResource, HttpRouteTree } from "rouzer/http"
import type { z } from "zod"

type UnionToIntersection<TUnion> = (
  TUnion extends unknown ? (value: TUnion) => void : never
) extends (value: infer TIntersection) => void
  ? TIntersection
  : never

/** Infers the exact backend route tree produced by composing route fragments. */
export type ComposeBackendRoutes<TRoutes extends readonly HttpRouteTree[]> = UnionToIntersection<
  TRoutes[number]
>

export type BackendEventEnvelope<
  TName extends string = string,
  TPayload = unknown,
  TProvenance = unknown,
> = {
  readonly name: TName
  readonly payload: TPayload
  readonly provenance?: TProvenance
}

export type BackendEventDefinition<TPayload = unknown, TProvenance = unknown> = {
  readonly payload: z.ZodType<TPayload>
  readonly provenance?: z.ZodType<TProvenance>
}

export type BackendEventSourceDefinition<
  TEventName extends string = string,
  TPrincipal = unknown,
  TEvent extends BackendEventEnvelope<TEventName> = BackendEventEnvelope<TEventName>,
> = {
  readonly produces: readonly TEventName[]
  readonly authorize: (input: {
    readonly principal: TPrincipal
    readonly event: TEvent
  }) => boolean | Promise<boolean>
}

export type BackendEventDefinitions = Record<string, BackendEventDefinition<any, any>>

export type BackendEventSourceDefinitions = Record<
  string,
  BackendEventSourceDefinition<string, any, any>
>

export type BackendPluginDefinition<
  TName extends string = string,
  TRoutes extends HttpRouteTree = HttpRouteTree,
  TEvents extends BackendEventDefinitions = BackendEventDefinitions,
  TSources extends BackendEventSourceDefinitions = BackendEventSourceDefinitions,
> = {
  readonly name: TName
  readonly routes?: TRoutes
  readonly events?: TEvents
  readonly eventSources?: TSources
}

export type BackendEventPublisher = {
  readonly publish: (input: {
    readonly source: string
    readonly event: BackendEventEnvelope
  }) => Promise<void>
}

/** Infers the exact backend event definition map produced by composing event fragments. */
export type ComposeBackendEvents<TEvents extends readonly BackendEventDefinitions[]> =
  UnionToIntersection<TEvents[number]>

/** Infers the exact backend event source map produced by composing source fragments. */
export type ComposeBackendEventSources<TSources extends readonly BackendEventSourceDefinitions[]> =
  UnionToIntersection<TSources[number]>

export type ComposeBackendPlugins<TPlugins extends readonly BackendPluginDefinition[]> = {
  readonly routes: ComposeBackendRoutes<ExtractBackendPluginRoutes<TPlugins>>
  readonly events: ComposeBackendEvents<ExtractBackendPluginEvents<TPlugins>>
  readonly eventSources: ComposeBackendEventSources<ExtractBackendPluginSources<TPlugins>>
}

type ExtractBackendPluginRoutes<TPlugins extends readonly BackendPluginDefinition[]> = {
  readonly [TIndex in keyof TPlugins]: TPlugins[TIndex] extends { readonly routes?: infer TRoutes }
    ? TRoutes extends HttpRouteTree
      ? TRoutes
      : {}
    : {}
}

type ExtractBackendPluginEvents<TPlugins extends readonly BackendPluginDefinition[]> = {
  readonly [TIndex in keyof TPlugins]: TPlugins[TIndex] extends { readonly events?: infer TEvents }
    ? TEvents extends BackendEventDefinitions
      ? TEvents
      : {}
    : {}
}

type ExtractBackendPluginSources<TPlugins extends readonly BackendPluginDefinition[]> = {
  readonly [TIndex in keyof TPlugins]: TPlugins[TIndex] extends {
    readonly eventSources?: infer TSources
  }
    ? TSources extends BackendEventSourceDefinitions
      ? TSources
      : {}
    : {}
}

/** Preserves the exact Rouzer route tree object for backend route inference. */
export function defineBackendRoutes<const TRoutes extends HttpRouteTree>(routes: TRoutes) {
  return routes
}

/** Preserves one feature-owned backend plugin contribution object. */
export function defineBackendPlugin<const TPlugin extends BackendPluginDefinition>(
  plugin: TPlugin,
) {
  return plugin
}

/** Preserves the exact backend event definition object for backend event inference. */
export function defineBackendEvents<const TEvents extends BackendEventDefinitions>(
  events: TEvents,
) {
  return events
}

/** Preserves the exact backend event source object for backend source inference. */
export function defineBackendEventSources<const TSources extends BackendEventSourceDefinitions>(
  sources: TSources,
) {
  return sources
}

/** Combines backend route fragments and rejects ambiguous action ownership. */
export function composeBackendRoutes<const TRoutes extends readonly HttpRouteTree[]>(
  routes: TRoutes,
) {
  const composed: HttpRouteTree = {}

  for (const routeTree of routes) {
    mergeRouteTree(composed, routeTree, [])
  }

  return composed as ComposeBackendRoutes<TRoutes>
}

/** Combines backend event fragments and rejects ambiguous event ownership. */
export function composeBackendEvents<const TEvents extends readonly BackendEventDefinitions[]>(
  eventSets: TEvents,
) {
  const composed: BackendEventDefinitions = {}

  for (const events of eventSets) {
    for (const [name, definition] of Object.entries(events)) {
      if (composed[name]) {
        throw new Error(`Duplicate backend event: ${name}`)
      }

      composed[name] = definition
    }
  }

  return composed as ComposeBackendEvents<TEvents>
}

/** Combines backend event source fragments and validates source/event compatibility. */
export function composeBackendEventSources<
  const TSources extends readonly BackendEventSourceDefinitions[],
>(sourceSets: TSources, events: BackendEventDefinitions) {
  const composed: BackendEventSourceDefinitions = {}

  for (const sources of sourceSets) {
    for (const [name, source] of Object.entries(sources)) {
      if (composed[name]) {
        throw new Error(`Duplicate backend event source: ${name}`)
      }

      for (const eventName of source.produces) {
        if (!events[eventName]) {
          throw new Error(`Backend event source ${name} produces unknown event: ${eventName}`)
        }
      }

      composed[name] = source
    }
  }

  return composed as ComposeBackendEventSources<TSources>
}

/** Composes backend plugin contributions and rejects ambiguous plugin ownership. */
export function composeBackendPlugins<const TPlugins extends readonly BackendPluginDefinition[]>(
  plugins: TPlugins,
) {
  const pluginNames = new Set<string>()
  const routes: HttpRouteTree[] = []
  const eventSets: BackendEventDefinitions[] = []
  const sourceSets: BackendEventSourceDefinitions[] = []

  for (const plugin of plugins) {
    if (pluginNames.has(plugin.name)) {
      throw new Error(`Duplicate backend plugin: ${plugin.name}`)
    }
    pluginNames.add(plugin.name)

    routes.push(plugin.routes ?? {})
    eventSets.push(plugin.events ?? {})
    sourceSets.push(plugin.eventSources ?? {})
  }

  const events = composeBackendEvents(eventSets)

  return {
    routes: composeBackendRoutes(routes),
    events,
    eventSources: composeBackendEventSources(sourceSets, events),
  } as ComposeBackendPlugins<TPlugins>
}

function mergeRouteTree(target: HttpRouteTree, source: HttpRouteTree, path: readonly string[]) {
  for (const [key, sourceNode] of Object.entries(source)) {
    const existingNode = target[key]
    if (!existingNode) {
      target[key] = sourceNode
      continue
    }

    target[key] = mergeRouteNode(existingNode, sourceNode, [...path, key])
  }
}

function mergeRouteNode(target: HttpNode, source: HttpNode, path: readonly string[]): HttpNode {
  if (target.kind === "resource" && source.kind === "resource") {
    assertSameResourcePath(target, source, path)
    mergeRouteTree(target.children, source.children, path)
    return target
  }

  throw new Error(`Duplicate backend route: ${path.join(".")}`)
}

function assertSameResourcePath(
  target: HttpResource,
  source: HttpResource,
  path: readonly string[],
) {
  if (formatRoutePath(target) !== formatRoutePath(source)) {
    throw new Error(`Conflicting backend resource path: ${path.join(".")}`)
  }
}

function formatRoutePath(node: HttpResource | HttpAction) {
  return node.path ? String(node.path) : ""
}

export {
  $type,
  createClient as createBackendClient,
  createRouter as createBackendRouter,
  type RouzerClient,
} from "rouzer"
export * as http from "rouzer/http"
export type { HttpRouteTree } from "rouzer/http"
