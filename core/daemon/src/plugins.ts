import { getDefaultDaemonPluginComposition } from "@goddard-ai/default-features/daemon"

import {
  openDaemonStore,
  type DaemonStore,
  type StoreConnectionOptions,
} from "./persistence/store.ts"

export type { DaemonStore, StoreConnectionOptions } from "./persistence/store.ts"

type ComposedDaemonPluginComposition = ReturnType<typeof getDefaultDaemonPluginComposition>
type ComposedDaemonDbSchema = ComposedDaemonPluginComposition["db"]["schema"]

/** Store handle opened against this daemon build's composed plugin schema. */
export type ComposedDaemonStore = DaemonStore<ComposedDaemonDbSchema>

/** Returns the statically composed daemon feature surface used by this daemon build. */
export function getDaemonPluginComposition() {
  return getDefaultDaemonPluginComposition()
}

/** Opens a daemon store using the schema contributed by the composed daemon plugins. */
export function openComposedDaemonStore(connection?: StoreConnectionOptions): ComposedDaemonStore {
  return openDaemonStore(getDaemonPluginComposition().db, connection)
}
