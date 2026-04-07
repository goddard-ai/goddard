import type { GoddardClient } from "@goddard-ai/sdk"
import type { DaemonRequestName, DaemonRequestPayload } from "~/shared/desktop-rpc.ts"
import { daemonSend } from "./desktop-host.ts"

/** Browser-safe daemon client adapter backed by the Electrobun Bun host bridge. */
export const desktopDaemonClient: GoddardClient = {
  send: async (
    name: DaemonRequestName,
    payload: DaemonRequestPayload<DaemonRequestName> = undefined,
  ) => await daemonSend(name, payload),
  subscribe: async () => {
    throw new Error(
      "Daemon stream subscriptions over the Electrobun bridge are not implemented yet.",
    )
  },
}
