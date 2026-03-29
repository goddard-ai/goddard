import { GoddardSdk as BaseGoddardSdk } from "../sdk.ts"
import { resolveNodeDaemonClient, type NodeDaemonClientOptions } from "./client.ts"

export * from "./client.ts"

/** Goddard SDK entry point for Node.js hosts that only injects the Node daemon client. */
export class GoddardSdk extends BaseGoddardSdk {
  constructor(options: NodeDaemonClientOptions = {}) {
    super({ client: resolveNodeDaemonClient(options) })
  }
}
