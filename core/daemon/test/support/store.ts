import { getDaemonPluginComposition, type ComposedDaemonStore } from "../../src/plugins.ts"
import { openDaemonStore, type StoreConnectionOptions } from "../../src/persistence/store.ts"

/** Opens a fresh daemon store for tests using the composed daemon plugin schema. */
export function resetComposedDaemonStore(connection?: StoreConnectionOptions): ComposedDaemonStore {
  return openDaemonStore(getDaemonPluginComposition().db, connection)
}

export type { ComposedDaemonStore }
