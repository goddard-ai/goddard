import { GoddardSdk } from "@goddard-ai/sdk"

import { desktopDaemonClient } from "./daemon-client.ts"

/** Shared browser-side SDK instance backed by the Electrobun daemon bridge. */
export const goddardSdk = new GoddardSdk({
  client: desktopDaemonClient,
})
