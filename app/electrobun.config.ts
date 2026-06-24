import { readFileSync } from "node:fs"
import { join } from "node:path"
import type { ElectrobunConfig } from "electrobun"

import pkg from "./package.json" with { type: "json" }

const shouldBuildEmbeddedRuntime = process.env.NODE_ENV !== "development"
const runtimeBunVersion = readWorkspaceCatalogBunVersion()

/** Electrobun build config for the desktop host and Vite-produced webview assets. */
export default {
  app: {
    name: "Goddard",
    identifier: "app.goddardai.org",
    version: pkg.version,
  },
  scripts: {
    ...(shouldBuildEmbeddedRuntime && {
      preBuild: "./scripts/prepare-embedded-runtime.ts",
    }),
  },
  build: {
    bunVersion: runtimeBunVersion,
    bun: {
      entrypoint: "src/bun/index.ts",
      tsconfig: "src/bun/tsconfig.json",
    },
    copy: {
      "build/views/main": "views/main",
      ...(shouldBuildEmbeddedRuntime && {
        ".generated/embedded-runtime": "embedded-runtime",
      }),
    },
    watchIgnore: ["src", "src/**", ".generated", ".generated/**"],
    mac: {
      bundleCEF: true,
      defaultRenderer: "cef",
      icons: "assets/icon.iconset",
    },
    win: {
      bundleCEF: true,
      defaultRenderer: "cef",
      icon: "assets/icon.png",
    },
    linux: {
      bundleCEF: true,
      defaultRenderer: "cef",
      icon: "assets/icon.png",
    },
  },
  release: {
    baseUrl: `https://github.com/${process.env.GITHUB_REPOSITORY ?? "goddard-ai/goddard"}/releases/latest/download`,
  },
} satisfies ElectrobunConfig

function readWorkspaceCatalogBunVersion() {
  const workspaceYaml = readFileSync(join(import.meta.dirname, "..", "pnpm-workspace.yaml"), "utf8")
  const catalogSection = /(?:^|\n)catalog:\n((?:^[ \t].*\n?)*)/m.exec(workspaceYaml)?.[1]
  const version = catalogSection && /^\s{2}bun:\s*(\S+)\s*$/m.exec(catalogSection)?.[1]

  if (!version) {
    throw new Error("pnpm-workspace.yaml#catalog.bun must pin the Bun runtime version")
  }

  return version
}
