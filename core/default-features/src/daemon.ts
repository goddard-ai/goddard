import { actionPlugin } from "@goddard-ai/action/daemon"
import { agentPlugin } from "@goddard-ai/agent/daemon"
import { authPlugin } from "@goddard-ai/auth/daemon"
import { composePlugins } from "@goddard-ai/daemon-plugin"
import { fileSearchPlugin } from "@goddard-ai/file-search/daemon"
import { inboxPlugin } from "@goddard-ai/inbox/daemon"
import { loopPlugin } from "@goddard-ai/loop/daemon"
import { pullRequestPlugin } from "@goddard-ai/pull-request/daemon"
import { reviewSessionPlugin } from "@goddard-ai/review-session/daemon"
import { sessionPlugin } from "@goddard-ai/session/daemon"
import { terminalPlugin } from "@goddard-ai/terminal/daemon"
import { workforcePlugin } from "@goddard-ai/workforce/daemon"

const defaultDaemonPlugins = [
  actionPlugin,
  authPlugin,
  fileSearchPlugin,
  agentPlugin,
  sessionPlugin,
  inboxPlugin,
  pullRequestPlugin,
  reviewSessionPlugin,
  loopPlugin,
  terminalPlugin,
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
