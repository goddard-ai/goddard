/** Environment variables recognized by the daemon runtime. */
export type DaemonRuntimeEnv = Record<string, string | undefined>
/** Explicit daemon launch settings accepted from CLI or tests before env/default resolution. */
export type DaemonRuntimeConfigInput = {
  baseUrl?: string
  socketPath?: string
  agentBinDir?: string
  env?: DaemonRuntimeEnv
}
/** Fully resolved daemon runtime contract shared across the daemon entry points. */
export type ResolvedDaemonRuntimeConfig = {
  baseUrl: string
  socketPath: string
  daemonUrl: string
  agentBinDir: string
}
export declare function resolveDaemonRuntimeConfig(
  input?: DaemonRuntimeConfigInput,
): ResolvedDaemonRuntimeConfig
export declare function prependAgentBinToPath(
  agentBinDir: string,
  env?: Record<string, string>,
): Record<string, string>
//# sourceMappingURL=config.d.ts.map
