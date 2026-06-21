import { GoddardSdk } from "@goddard-ai/sdk"

import { browserDaemonClient } from "./daemon-client.ts"

/** Shared browser-side SDK instance backed by direct loopback daemon IPC. */
export const goddardSdk = new GoddardSdk({
  client: browserDaemonClient,
})
