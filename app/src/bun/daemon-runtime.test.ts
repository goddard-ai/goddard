import { join } from "node:path"
import { expect, test } from "vitest"

import {
  bindBunRuntimeLauncher,
  createDaemonRunArgs,
  resolveInstalledNativeRuntimePaths,
  type PreparedDaemonRuntime,
} from "./daemon-runtime-launch.ts"
import type { EmbeddedRuntimeManifest } from "./embedded-runtime-manifest.ts"

test("installed native runtime paths resolve relative manifest paths under the install dir", () => {
  expect(
    resolveInstalledNativeRuntimePaths(
      createEmbeddedRuntimeManifest({
        nativeLibraries: {
          libgit2: {
            target: "bun-darwin-arm64",
            path: "daemon/native/libgit2/libgit2.dylib",
            version: "1.9.0",
            sha256: "abc123",
          },
        },
      }),
      "/installed/runtime",
    ),
  ).toEqual({
    gitLibgit2Path: join("/installed/runtime", "daemon", "native", "libgit2", "libgit2.dylib"),
  })
})

test("installed native runtime paths are absent without native library metadata", () => {
  expect(
    resolveInstalledNativeRuntimePaths(createEmbeddedRuntimeManifest(), "/installed/runtime"),
  ).toEqual({})
})

test("shared Bun launchers preserve their staged payload path when pinned", () => {
  expect(
    bindBunRuntimeLauncher(
      [
        "#!/bin/sh",
        'launcher_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"',
        'exec "${GODDARD_BUN_RUNTIME:-bun}" "$launcher_dir/../runtime/main.mjs" "$@"',
        "",
      ].join("\n"),
      "/Applications/Goddard's Runtime/bun",
    ),
  ).toContain(
    `exec '/Applications/Goddard'\\''s Runtime/bun' "$launcher_dir/../runtime/main.mjs" "$@"`,
  )
})

test("daemon runtime args omit Git libgit2 path when no native library is installed", () => {
  expect(
    createDaemonRunArgs({
      runtime: createPreparedRuntime(),
      baseUrl: "https://example.test/api",
      daemonPort: 49827,
    }),
  ).toEqual([
    "/runtime/bin/goddard-daemon",
    "run",
    "--base-url",
    "https://example.test/api",
    "--port",
    "49827",
    "--agent-bin-dir",
    "/runtime/agent-bin",
  ])
})

test("daemon runtime args include the installed Git libgit2 path", () => {
  expect(
    createDaemonRunArgs({
      runtime: createPreparedRuntime({
        gitLibgit2Path: "/runtime/native/libgit2/libgit2.dylib",
      }),
      baseUrl: "https://example.test/api",
      daemonPort: 49827,
      dataProfile: "development",
    }),
  ).toEqual([
    "/runtime/bin/goddard-daemon",
    "run",
    "--base-url",
    "https://example.test/api",
    "--port",
    "49827",
    "--agent-bin-dir",
    "/runtime/agent-bin",
    "--git-libgit2-path",
    "/runtime/native/libgit2/libgit2.dylib",
    "--data-profile",
    "development",
  ])
})

function createPreparedRuntime(
  overrides: Partial<PreparedDaemonRuntime> = {},
): PreparedDaemonRuntime {
  return {
    daemonRootDir: "/runtime",
    agentBinDir: "/runtime/agent-bin",
    daemonExecutablePath: "/runtime/bin/goddard-daemon",
    runtimeHash: "runtime-hash",
    ...overrides,
  }
}

function createEmbeddedRuntimeManifest(
  daemon: Partial<EmbeddedRuntimeManifest["daemon"]> = {},
): EmbeddedRuntimeManifest {
  return {
    formatVersion: 1,
    target: {
      os: "macos",
      arch: "arm64",
      bunTarget: "bun-darwin-arm64",
    },
    daemon: {
      version: "0.1.0",
      runtimeHash: "runtime-hash",
      executablePath: "daemon/bin/goddard-daemon",
      agentBinDir: "daemon/agent-bin",
      helperPaths: {
        goddard: "daemon/agent-bin/goddard",
        workforce: "daemon/agent-bin/workforce",
      },
      ...daemon,
    },
    serviceman: {
      version: "v0.9.5",
      launcherPath: "serviceman/bin/serviceman",
      shareDir: "serviceman/share/serviceman",
    },
  }
}
