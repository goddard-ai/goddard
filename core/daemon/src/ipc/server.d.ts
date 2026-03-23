import type { BackendPrClient, DaemonServer, DaemonServerDeps } from "./types.ts"
export declare function startDaemonServer(
  client: BackendPrClient,
  options?: {
    socketPath?: string
    agentBinDir?: string
  },
  deps?: DaemonServerDeps,
): Promise<DaemonServer>
//# sourceMappingURL=server.d.ts.map
