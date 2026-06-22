import { actionPlugin } from "@goddard-ai/action/daemon"
import { managedAgentPlugin } from "@goddard-ai/agent/daemon"
import { authPlugin } from "@goddard-ai/auth/daemon"
import { composePlugins } from "@goddard-ai/daemon-plugin"
import { fileSearchPlugin } from "@goddard-ai/file-search/daemon"
import { inboxPlugin } from "@goddard-ai/inbox/daemon"
import { loopPlugin } from "@goddard-ai/loop/daemon"
import { pullRequestPlugin } from "@goddard-ai/pull-request/daemon"
import { reviewSessionPlugin } from "@goddard-ai/review-session/daemon"
import { sessionPlugin } from "@goddard-ai/session/daemon"
import { workforcePlugin } from "@goddard-ai/workforce/daemon"

const defaultDaemonPlugins = [
  actionPlugin,
  authPlugin,
  fileSearchPlugin,
  managedAgentPlugin,
  sessionPlugin,
  inboxPlugin,
  pullRequestPlugin,
  reviewSessionPlugin,
  loopPlugin,
  workforcePlugin,
] as const

function composeDefaultDaemonPlugins() {
  return composePlugins(defaultDaemonPlugins)
}

let defaultDaemonPluginsComposition: ReturnType<typeof composeDefaultDaemonPlugins> | null = null

/** Returns the statically composed default daemon feature surface. */
export function getDefaultDaemonPluginComposition() {
  defaultDaemonPluginsComposition ??= composeDefaultDaemonPlugins()

  return defaultDaemonPluginsComposition
}
