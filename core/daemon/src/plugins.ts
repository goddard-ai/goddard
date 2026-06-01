import { actionPlugin } from "@goddard-ai/action/daemon"
import { adapterPlugin } from "@goddard-ai/adapter/daemon"
import { authPlugin } from "@goddard-ai/auth/daemon"
import { composePlugins } from "@goddard-ai/daemon-plugin"
import { inboxPlugin } from "@goddard-ai/inbox/daemon"
import { loopPlugin } from "@goddard-ai/loop/daemon"
import { pullRequestPlugin } from "@goddard-ai/pull-request/daemon"
import { reviewSessionPlugin } from "@goddard-ai/review-session/daemon"
import { sessionPlugin } from "@goddard-ai/session/daemon"
import { workforcePlugin } from "@goddard-ai/workforce/daemon"

import {
  openDaemonStore,
  type DaemonStore,
  type StoreConnectionOptions,
} from "./persistence/store.ts"

export type { DaemonStore, StoreConnectionOptions } from "./persistence/store.ts"

const daemonPluginList = [
  actionPlugin,
  adapterPlugin,
  authPlugin,
  sessionPlugin,
  inboxPlugin,
  pullRequestPlugin,
  reviewSessionPlugin,
  loopPlugin,
  workforcePlugin,
] as const

function composeDaemonPlugins() {
  return composePlugins(daemonPluginList)
}

type ComposedDaemonPluginComposition = ReturnType<typeof composeDaemonPlugins>
type ComposedDaemonDbSchema = ComposedDaemonPluginComposition["db"]

/** Store handle opened against this daemon build's composed plugin schema. */
export type ComposedDaemonStore = DaemonStore<ComposedDaemonDbSchema>

let daemonPlugins: ComposedDaemonPluginComposition | null = null

/** Returns the statically composed daemon feature surface used by this daemon build. */
export function getDaemonPluginComposition() {
  if (!daemonPlugins) {
    daemonPlugins = composeDaemonPlugins()
  }

  return daemonPlugins
}

/** Opens a daemon store using the schema contributed by the composed daemon plugins. */
export function openComposedDaemonStore(connection?: StoreConnectionOptions): ComposedDaemonStore {
  return openDaemonStore(getDaemonPluginComposition().db, connection)
}

/** Opens a fresh daemon store for tests using the composed daemon plugin schema. */
export function resetComposedDaemonStore(connection?: StoreConnectionOptions): ComposedDaemonStore {
  return openDaemonStore(getDaemonPluginComposition().db, connection)
}
