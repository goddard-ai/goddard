import { actionIpcRoutes } from "@goddard-ai/action/daemon-ipc"
import { adapterIpcRoutes } from "@goddard-ai/adapter/daemon-ipc"
import { authIpcRoutes } from "@goddard-ai/auth/daemon-ipc"
import { inboxIpcRoutes } from "@goddard-ai/inbox/daemon-ipc"
import {
  $type,
  composeIpcRoutes,
  defineIpcRoutes,
  getResponsePluginMarkerId,
  http,
  type HttpRouteTree,
} from "@goddard-ai/ipc"
import { loopIpcRoutes } from "@goddard-ai/loop/daemon-ipc"
import { pullRequestIpcRoutes } from "@goddard-ai/pull-request/daemon-ipc"
import { sessionIpcRoutes } from "@goddard-ai/session/daemon-ipc"
import { workforceIpcRoutes } from "@goddard-ai/workforce/daemon-ipc"

const coreDaemonIpcRoutes = defineIpcRoutes({
  daemon: http.resource("daemon", {
    health: http.get("health", {
      response: $type<{ ok: boolean }>(),
    }),
  }),
})

/** IPC route tree shared by daemon clients and server composition roots. */
export const daemonIpcRoutes = composeIpcRoutes([
  coreDaemonIpcRoutes,
  actionIpcRoutes,
  adapterIpcRoutes,
  authIpcRoutes,
  inboxIpcRoutes,
  loopIpcRoutes,
  pullRequestIpcRoutes,
  sessionIpcRoutes,
  workforceIpcRoutes,
])

/** Compatibility schema for the old transport while the daemon server moves to Rouzer. */
export const daemonIpcSchema = createLegacySchemaFromRoutes(daemonIpcRoutes)

function createLegacySchemaFromRoutes(routes: HttpRouteTree) {
  const requests: Record<string, unknown> = {}
  const streams: Record<string, unknown> = {}

  function visit(node: unknown, path: string[]) {
    if (!node || typeof node !== "object") {
      return
    }

    if ("kind" in node && node.kind === "resource" && "children" in node) {
      for (const [key, child] of Object.entries(node.children as Record<string, unknown>)) {
        visit(child, [...path, key])
      }
      return
    }

    if ("kind" in node && node.kind === "action" && "schema" in node) {
      const schema = node.schema as {
        readonly body?: unknown
        readonly query?: unknown
        readonly response?: unknown
      }
      const name = path.join(".")
      if (schema.query && isNdjsonResponse(schema.response)) {
        streams[name] = {
          payload: schema.response,
          filter: schema.query,
        }
        return
      }

      requests[name] = {
        payload: schema.body,
        response: schema.response,
      }
    }
  }

  for (const [key, child] of Object.entries(routes)) {
    visit(child, [key])
  }

  return { requests, streams }
}

function isNdjsonResponse(response: unknown) {
  return getResponsePluginMarkerId(response) === "rouzer/ndjson"
}
