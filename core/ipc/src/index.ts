export {
  createClient,
  getResponsePluginMarkerId,
  metadata,
  type RouzerClient,
  type RouzerClientHook as IpcClientHook,
  type RouzerClientHookEvent as IpcClientHookEvent,
  type RouteMetadata,
  type RouteRequestHandlerMap,
} from "rouzer"

export type { HttpRouteTree } from "rouzer/http"

export * as http from "rouzer/http"
export * as ndjson from "rouzer/ndjson"

export * from "./errors.ts"
export * from "./metadata.ts"
export * from "./routes.ts"
export * from "./schema.ts"
