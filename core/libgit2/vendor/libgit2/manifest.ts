export const nativeLibgit2Manifest = {
  name: "goddard-libgit2-native-artifacts",
  version: 1,
  targets: {
    "darwin-arm64": {
      platform: "darwin",
      arch: "arm64",
      bunTarget: "bun-darwin-arm64",
      library: "lib/libgit2.dylib",
    },
  },
} as const

export type NativeLibgit2Target = keyof typeof nativeLibgit2Manifest.targets

export function nativeLibgit2TargetForBunTarget(bunTarget: string) {
  const match = Object.entries(nativeLibgit2Manifest.targets).find(
    ([, target]) => target.bunTarget === bunTarget,
  )
  return (match?.[0] as NativeLibgit2Target | undefined) ?? null
}
