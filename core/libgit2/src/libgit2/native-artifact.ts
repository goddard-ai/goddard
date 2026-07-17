import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"

import { nativeLibgit2Manifest, type NativeLibgit2Target } from "../../vendor/libgit2/manifest.ts"

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

  const artifact = nativeLibgit2Manifest.targets[target].library
  const roots = [
    resolve(input.moduleDir ?? moduleDir, "../../vendor/libgit2"),
    resolve(input.cwd ?? process.cwd(), "core/libgit2/vendor/libgit2"),
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
