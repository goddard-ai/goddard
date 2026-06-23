import { getResponsePluginMarkerId } from "rouzer"
import type { HttpAction, HttpNode, HttpResource, HttpRouteTree } from "rouzer/http"

type UnionToIntersection<TUnion> = (
  TUnion extends unknown ? (value: TUnion) => void : never
) extends (value: infer TIntersection) => void
  ? TIntersection
  : never

/** Infers the exact route tree produced by composing IPC route fragments. */
export type ComposeIpcRoutes<TRoutes extends readonly HttpRouteTree[]> = UnionToIntersection<
  TRoutes[number]
>

/** Preserves the exact Rouzer route tree object for daemon IPC route inference. */
export function defineIpcRoutes<const TRoutes extends HttpRouteTree>(routes: TRoutes) {
  return routes
}

/** Describes one leaf action discovered by walking an IPC route tree. */
export type IpcRouteAction = {
  keyPath: readonly string[]
  commandPath: readonly string[]
  httpPath: readonly string[]
  action: HttpAction
  requestInput: "body" | "query" | null
  streamsNdjson: boolean
}

/** Combines daemon IPC route fragments and rejects ambiguous action ownership. */
export function composeIpcRoutes<const TRoutes extends readonly HttpRouteTree[]>(routes: TRoutes) {
  const composed: HttpRouteTree = {}

  for (const routeTree of routes) {
    mergeRouteTree(composed, routeTree, [])
  }

  return composed as ComposeIpcRoutes<TRoutes>
}

/** Walks one IPC route tree and returns each concrete action with its route metadata. */
export function listIpcRouteActions(routes: HttpRouteTree): IpcRouteAction[] {
  const actions: IpcRouteAction[] = []
  collectIpcRouteActions(routes, actions)
  return actions
}

function mergeRouteTree(target: HttpRouteTree, source: HttpRouteTree, path: readonly string[]) {
  for (const [key, sourceNode] of Object.entries(source)) {
    const existingNode = target[key]
    if (!existingNode) {
      target[key] = cloneRouteNode(sourceNode)
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

  throw new Error(`Duplicate IPC route: ${path.join(".")}`)
}

function assertSameResourcePath(
  target: HttpResource,
  source: HttpResource,
  path: readonly string[],
) {
  if (formatRoutePath(target) !== formatRoutePath(source)) {
    throw new Error(`Conflicting IPC resource path: ${path.join(".")}`)
  }
}

function formatRoutePath(node: HttpResource | HttpAction) {
  return node.path ? String(node.path) : ""
}

function collectIpcRouteActions(
  routes: HttpRouteTree,
  actions: IpcRouteAction[],
  context: {
    keyPath: readonly string[]
    httpPath: readonly string[]
  } = { keyPath: [], httpPath: [] },
) {
  for (const [key, node] of Object.entries(routes)) {
    const keyPath = [...context.keyPath, key]
    const httpPath = appendRoutePath(context.httpPath, formatRoutePath(node))

    if (node.kind === "resource") {
      collectIpcRouteActions(node.children, actions, { keyPath, httpPath })
      continue
    }

    actions.push({
      keyPath,
      commandPath: keyPath,
      httpPath,
      action: node,
      requestInput: getRouteRequestInput(node),
      streamsNdjson: getResponsePluginMarkerId(node.schema.response) === "rouzer/ndjson",
    })
  }
}

function appendRoutePath(path: readonly string[], next: string) {
  const normalized = next.replace(/^\/+/, "")
  return normalized ? [...path, normalized] : path
}

function getRouteRequestInput(action: HttpAction): IpcRouteAction["requestInput"] {
  if ("body" in action.schema) {
    return "body"
  }
  if ("query" in action.schema) {
    return "query"
  }
  return null
}

function cloneRouteTree(routeTree: HttpRouteTree) {
  const cloned: HttpRouteTree = {}

  for (const [key, node] of Object.entries(routeTree)) {
    cloned[key] = cloneRouteNode(node)
  }

  return cloned
}

function cloneRouteNode(node: HttpNode): HttpNode {
  if (node.kind === "resource") {
    return {
      ...node,
      children: cloneRouteTree(node.children),
    }
  }

  return { ...node }
}
