import { authBackendPlugin } from "@goddard-ai/auth/backend"
import { composeBackendPlugins } from "@goddard-ai/backend-plugin"
import { githubBackendPlugin } from "@goddard-ai/github/backend"
import { pullRequestBackendPlugin } from "@goddard-ai/pull-request/backend"
import { remoteRepoBackendPlugin } from "@goddard-ai/remote-repo/backend"

const defaultBackendPlugins = [
  authBackendPlugin,
  githubBackendPlugin,
  remoteRepoBackendPlugin,
  pullRequestBackendPlugin,
] as const

function composeDefaultBackendPlugins() {
  return composeBackendPlugins(defaultBackendPlugins)
}

let defaultBackendPluginsComposition: ReturnType<typeof composeDefaultBackendPlugins> | null = null

/** Returns the statically composed default backend feature surface. */
export function getDefaultBackendPluginComposition() {
  defaultBackendPluginsComposition ??= composeDefaultBackendPlugins()

  return defaultBackendPluginsComposition
}
