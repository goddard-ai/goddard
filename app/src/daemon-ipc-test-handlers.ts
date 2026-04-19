import type { Handlers } from "@goddard-ai/ipc"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"

function createUnsupportedHandler(name: string) {
  return async () => {
    throw new Error(`Missing daemon IPC test stub for "${name}".`)
  }
}

export const daemonIpcTestHandlers = new Proxy(
  {},
  {
    get: (_target, key) => createUnsupportedHandler(String(key)),
  },
) as Handlers<typeof daemonIpcSchema>
