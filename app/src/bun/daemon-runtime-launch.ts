import { join } from "node:path"

import type { EmbeddedRuntimeManifest } from "./embedded-runtime-manifest.ts"

export type PreparedDaemonRuntime = {
  daemonRootDir: string
  agentBinDir: string
  daemonExecutablePath: string
  gitLibgit2Path?: string
  runtimeHash: string
}

export function resolveInstalledNativeRuntimePaths(
  manifest: EmbeddedRuntimeManifest,
  installDir: string,
) {
  return {
    ...(manifest.daemon.nativeLibraries?.libgit2 && {
      gitLibgit2Path: join(installDir, manifest.daemon.nativeLibraries.libgit2.path),
    }),
  }
}

/** Pins one generated shared-Bun launcher to the executable bundled with the desktop app. */
export function bindBunRuntimeLauncher(source: string, bunExecutablePath: string) {
  const placeholder = '"${GODDARD_BUN_RUNTIME:-bun}"'
  if (!source.includes(placeholder)) {
    return null
  }
  return source.replace(placeholder, quoteShellLiteral(bunExecutablePath))
}

export function createDaemonRunArgs(input: {
  runtime: PreparedDaemonRuntime
  baseUrl: string
  daemonPort: number
  dataProfile?: string
}) {
  const args = [
    input.runtime.daemonExecutablePath,
    "run",
    "--base-url",
    input.baseUrl,
    "--port",
    String(input.daemonPort),
    "--agent-bin-dir",
    input.runtime.agentBinDir,
  ]

  if (input.runtime.gitLibgit2Path) {
    args.push("--git-libgit2-path", input.runtime.gitLibgit2Path)
  }

  if (input.dataProfile) {
    args.push("--data-profile", input.dataProfile)
  }

  return args
}

function quoteShellLiteral(value: string) {
  return `'${value.replaceAll("'", "'\\''")}'`
}
