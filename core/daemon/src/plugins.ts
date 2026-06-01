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

type DaemonPluginComposition = ReturnType<typeof composePlugins>

let daemonPlugins: DaemonPluginComposition | null = null

/** Returns the statically composed daemon feature surface used by this daemon build. */
export function getDaemonPluginComposition() {
  if (!daemonPlugins) {
    daemonPlugins = composePlugins(daemonPluginList)
  }

  return daemonPlugins
}

/** Opens a daemon store using the schema contributed by the composed daemon plugins. */
export function openComposedDaemonStore(connection?: StoreConnectionOptions): DaemonStore {
  return openDaemonStore(getDaemonPluginComposition().db, connection)
}

/** Opens a fresh daemon store for tests using the composed daemon plugin schema. */
export function resetComposedDaemonStore(connection?: StoreConnectionOptions): DaemonStore {
  return openDaemonStore(getDaemonPluginComposition().db, connection)
}
