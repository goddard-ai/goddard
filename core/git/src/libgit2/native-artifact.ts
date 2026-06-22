import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"

import nativeManifest from "../../../../native/libgit2/manifest.json" with { type: "json" }

type NativeLibgit2Target = keyof typeof nativeManifest.targets

type NativeLibgit2PathInput = {
  platform?: NodeJS.Platform
  arch?: string
  moduleDir?: string
  cwd?: string
}

const moduleDir = dirname(fileURLToPath(import.meta.url))

export function nativeLibgit2PathCandidates(input: NativeLibgit2PathInput = {}) {
  const target = nativeLibgit2TargetFor(
    input.platform ?? process.platform,
    input.arch ?? process.arch,
  )
  if (!target) {
    return []
  }

  const artifact = nativeManifest.targets[target].library
  const roots = [
    resolve(input.moduleDir ?? moduleDir, "../../../../native/libgit2"),
    resolve(input.cwd ?? process.cwd(), "native/libgit2"),
  ]

  return [...new Set(roots.map((root) => resolve(root, "dist", target, artifact)))]
}

function nativeLibgit2TargetFor(
  platform: NodeJS.Platform,
  arch: string,
): NativeLibgit2Target | null {
  if (platform === "darwin" && arch === "arm64") {
    return "darwin-arm64"
  }

  return null
}
