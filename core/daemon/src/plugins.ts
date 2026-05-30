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

import { configureDbSchema } from "./persistence/store.ts"

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
    configureDbSchema(daemonPlugins.db)
  }

  return daemonPlugins
}
