import { join } from "node:path"

import type { EmbeddedRuntimeManifest } from "./embedded-runtime-manifest.ts"

export type PreparedDaemonRuntime = {
  daemonRootDir: string
  agentBinDir: string
  daemonExecutablePath: string
  reviewSyncLibgit2Path?: string
  runtimeHash: string
}

export function resolveInstalledNativeRuntimePaths(
  manifest: EmbeddedRuntimeManifest,
  installDir: string,
) {
  return {
    ...(manifest.daemon.nativeLibraries?.libgit2 && {
      reviewSyncLibgit2Path: join(installDir, manifest.daemon.nativeLibraries.libgit2.path),
    }),
  }
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

  if (input.runtime.reviewSyncLibgit2Path) {
    args.push("--review-sync-libgit2-path", input.runtime.reviewSyncLibgit2Path)
  }

  if (input.dataProfile) {
    args.push("--data-profile", input.dataProfile)
  }

  return args
}
