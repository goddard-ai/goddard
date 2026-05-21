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
    // The schema-typed proxy keeps app test stubs aligned as daemon IPC methods change.
    get: (_target, key) => {
      if (key === "session.launchLease.release") {
        return createUnsupportedHandler("session.launchLease.release")
      }

      return createUnsupportedHandler(String(key))
    },
  },
) as Handlers<typeof daemonIpcSchema>
