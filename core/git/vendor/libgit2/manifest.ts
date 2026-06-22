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
