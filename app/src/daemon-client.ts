import type { DaemonIpcClient } from "@goddard-ai/daemon-client"
import { daemonSend } from "./desktop-host"
import type { DaemonRequestName, DaemonRequestPayload } from "./shared/desktop-rpc"

/** Browser-safe daemon client adapter backed by the Electrobun Bun host bridge. */
export const desktopDaemonClient: DaemonIpcClient = {
  send: async <Name extends DaemonRequestName>(name: Name, payload: DaemonRequestPayload<Name>) =>
    await daemonSend(name, payload),
  subscribe: async () => {
    throw new Error(
      "Daemon stream subscriptions over the Electrobun bridge are not implemented yet.",
    )
  },
}
