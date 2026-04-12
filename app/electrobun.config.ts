import type { ElectrobunConfig } from "electrobun"

import pkg from "./package.json" with { type: "json" }

/** Electrobun build config for the desktop host and Vite-produced webview assets. */
export default {
  app: {
    name: "Goddard",
    identifier: "app.goddardai.org",
    version: pkg.version,
  },
  scripts: {
    preBuild: "./scripts/prepare-embedded-runtime.ts",
  },
  build: {
    bun: {
      entrypoint: "src/bun/index.ts",
      tsconfig: "src/bun/tsconfig.json",
    },
    copy: {
      dist: "views/main",
      ".generated/embedded-runtime": "embedded-runtime",
    },
    watch: ["../core/daemon/src", "../core/daemon/scripts", "../core/daemon/package.json"],
    watchIgnore: ["dist/**", ".generated/**"],
    mac: {
      icons: "assets/icon.iconset",
    },
    win: {
      icon: "assets/icon.png",
    },
    linux: {
      icon: "assets/icon.png",
    },
  },
  release: {
    baseUrl: `https://github.com/${process.env.GITHUB_REPOSITORY ?? "goddard-ai/goddard"}/releases/latest/download`,
  },
} satisfies ElectrobunConfig
