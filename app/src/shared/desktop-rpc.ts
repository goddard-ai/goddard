import type { InferRequestPayload, InferResponseType, ValidRequestName } from "@goddard-ai/ipc"
import type { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import type { RPCSchema } from "electrobun/bun"

/** Daemon IPC schema type reused for webview-to-Bun request forwarding. */
type DaemonSchema = typeof daemonIpcSchema

/** Valid daemon IPC request names forwarded through the desktop host. */
export type DaemonRequestName = ValidRequestName<DaemonSchema>

/** Payload type for one forwarded daemon IPC request. */
export type DaemonRequestPayload<Name extends DaemonRequestName = DaemonRequestName> =
  InferRequestPayload<DaemonSchema, Name>

/** Response type for one forwarded daemon IPC request. */
export type DaemonRequestResponse<Name extends DaemonRequestName = DaemonRequestName> =
  InferResponseType<DaemonSchema, Name>

/** Bun-host RPC payload for forwarding one daemon IPC request. */
export type DaemonSendInput<Name extends DaemonRequestName = DaemonRequestName> = {
  name: Name
  payload: DaemonRequestPayload<Name>
}

/** Minimal runtime information exposed by the Electrobun Bun host. */
export type RuntimeInfo = {
  runtime: "electrobun"
}

/** Result returned after validating one candidate project root on the Bun side. */
export type ProjectInspection = {
  path: string
  name: string
}

/** Shared Electrobun RPC contract between the Bun host and the browser view. */
export type AppDesktopRpc = {
  bun: RPCSchema<{
    requests: {
      runtimeInfo: {
        params: {}
        response: RuntimeInfo
      }
      browseForProject: {
        params: {}
        response: { path: string | null }
      }
      inspectProject: {
        params: { path: string }
        response: ProjectInspection
      }
      daemonSend: {
        params: DaemonSendInput
        response: unknown
      }
    }
    messages: {}
  }>
  webview: RPCSchema<{
    requests: {}
    messages: {}
  }>
}
