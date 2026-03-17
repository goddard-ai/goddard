import { type IpcTransport } from "@goddard-ai/ipc"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"

export const IPC_STREAM_EVENT = "ipc://message"

type PluginStreamMessage = {
  subscriptionId?: unknown
  socketPath?: unknown
  name?: unknown
  payload?: unknown
}

export function createTauriTransport(socketPath: string): IpcTransport {
  return {
    async send(name, payload) {
      return await invoke("plugin:ipc|send", { socketPath, name, payload })
    },

    async subscribe(name, onMessage) {
      let activeSubscriptionId: string | null = null

      const unlisten = await listen<PluginStreamMessage>(IPC_STREAM_EVENT, (event) => {
        const message = event.payload
        if (
          typeof message.subscriptionId !== "string" ||
          message.subscriptionId !== activeSubscriptionId
        ) {
          return
        }
        if (message.socketPath !== socketPath || message.name !== name) {
          return
        }

        onMessage(message.payload)
      })

      try {
        activeSubscriptionId = await invoke<string>("plugin:ipc|subscribe", { socketPath, name })
      } catch (error) {
        unlisten()
        throw error
      }

      return async () => {
        const subscriptionId = activeSubscriptionId
        activeSubscriptionId = null
        unlisten()

        if (subscriptionId) {
          await invoke("plugin:ipc|unsubscribe", { subscriptionId })
        }
      }
    },
  }
}
