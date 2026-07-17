import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { vscodeTaskIpcRoutes } from "./daemon-ipc.ts"
import type {
  InspectVscodeTasksRequest,
  InspectVscodeTasksResponse,
  VscodeTaskCancelRequest,
  VscodeTaskConnectRequest,
  VscodeTaskConnectResponse,
  VscodeTaskDaemonEvent,
  VscodeTaskRunRequest,
  VscodeTaskRunResponse,
} from "./schema.ts"

export type VscodeTaskEventHandler = (event: VscodeTaskDaemonEvent) => void

export interface GoddardVscodeTaskConnection {
  readonly connectionId: VscodeTaskConnectResponse["connectionId"]
  run(input: Omit<VscodeTaskRunRequest, "connectionId">): Promise<VscodeTaskRunResponse>
  cancel(input: Omit<VscodeTaskCancelRequest, "connectionId">): Promise<void>
  disconnect(): Promise<void>
  subscribe(handler: VscodeTaskEventHandler): Promise<() => void>
}

export type GoddardVscodeTaskNamespace = {
  inspect(input: InspectVscodeTasksRequest): Promise<InspectVscodeTasksResponse>
  connect(input?: VscodeTaskConnectRequest): Promise<GoddardVscodeTaskConnection>
}

export const vscodeTaskSdkPlugin = defineSdkPlugin({
  name: "vscode-task",
  ipcRoutes: vscodeTaskIpcRoutes,
  wrap({ client }) {
    function createConnection(
      connectionId: VscodeTaskConnectResponse["connectionId"],
    ): GoddardVscodeTaskConnection {
      let streamController: AbortController | undefined

      return {
        connectionId,
        run: async (input) => client.vscodeTask.run({ ...input, connectionId }),
        cancel: async (input) => {
          await client.vscodeTask.cancel({ ...input, connectionId })
        },
        disconnect: async () => {
          try {
            await client.vscodeTask.disconnect({ connectionId })
          } finally {
            streamController?.abort()
            streamController = undefined
          }
        },
        subscribe: async (handler) => {
          const controller = new AbortController()
          const events = await client.vscodeTask.event(
            { connectionId },
            { signal: controller.signal },
          )
          streamController = controller

          void (async () => {
            try {
              for await (const event of events) {
                if (controller.signal.aborted) {
                  break
                }
                handler(event)
              }
            } catch (error) {
              if (!controller.signal.aborted) {
                throw error
              }
            } finally {
              if (streamController === controller) {
                streamController = undefined
              }
            }
          })()

          return () => {
            controller.abort()
          }
        },
      }
    }

    const vscodeTask: GoddardVscodeTaskNamespace = {
      inspect: async (input) => client.vscodeTask.inspect(input),
      connect: async (input = {}) => {
        const response = await client.vscodeTask.connect(input)
        return createConnection(response.connectionId)
      },
    }

    return { vscodeTask }
  },
})
