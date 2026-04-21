import type { ElectrobunConfig } from "electrobun"

import pkg from "./package.json" with { type: "json" }

const shouldBuildEmbeddedRuntime = process.env.NODE_ENV !== "development"

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
    bun: {
      entrypoint: "src/bun/index.ts",
      tsconfig: "src/bun/tsconfig.json",
    },
    copy: {
      dist: "views/main",
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
