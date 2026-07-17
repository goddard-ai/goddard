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
import { taskPlugin } from "@goddard-ai/task/daemon"
import { terminalPlugin } from "@goddard-ai/terminal/daemon"
import { vscodeTaskPlugin } from "@goddard-ai/vscode-task/daemon"
import { workforcePlugin } from "@goddard-ai/workforce/daemon"

const defaultDaemonPlugins = [
  actionPlugin,
  authPlugin,
  fileSearchPlugin,
  agentPlugin,
  sessionPlugin,
  taskPlugin,
  inboxPlugin,
  pullRequestPlugin,
  reviewSessionPlugin,
  loopPlugin,
  terminalPlugin,
  vscodeTaskPlugin,
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
