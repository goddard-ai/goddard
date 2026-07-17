import type { IpcClientHook } from "@goddard-ai/ipc"

import { formatClientIpcLogEvent } from "~/lib/ipc-log-event.ts"
import { getAppDebug } from "./logging.ts"

/** Creates the daemon IPC debug hook for the desktop host. */
export function createClientIpcLogHook(): IpcClientHook {
  const debug = getAppDebug("ipc.client")

  return (event) => {
    const { message, properties } = formatClientIpcLogEvent(event)
    debug(message, properties)
  }
}
