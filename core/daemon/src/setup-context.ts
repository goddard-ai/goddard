import { AsyncContext } from "@b9g/async-context"
import type { ConfigManager } from "./config-manager.ts"
import type { ResolvedDaemonRuntimeConfig } from "./config.ts"

/** Daemon setup context threaded through startup before long-lived managers capture dependencies. */
export type DaemonSetupContext = {
  runtime: ResolvedDaemonRuntimeConfig
  configManager: ConfigManager
}

const daemonSetupContext = new AsyncContext.Variable<DaemonSetupContext>()

/** Runs one daemon setup callback with the provided setup context installed. */
export function runWithDaemonSetupContext<T>(context: DaemonSetupContext, fn: () => T) {
  return daemonSetupContext.run(context, fn)
}

/** Returns the current daemon setup context when startup is running inside one async scope. */
export function getDaemonSetupContext() {
  return daemonSetupContext.get()
}
