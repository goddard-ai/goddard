import type { ElectrobunConfig } from "electrobun"
import pkg from "./package.json" with { type: "json" }

const releaseBaseUrl = `https://github.com/${process.env.GITHUB_REPOSITORY ?? "goddard-ai/goddard"}/releases/latest/download`

/** Electrobun build config for the desktop host and Vite-produced webview assets. */
export default {
  app: {
    name: "Goddard",
    identifier: "app.goddardai.org",
    version: pkg.version,
  },
  build: {
    bun: {
      entrypoint: "src/bun/index.ts",
    },
    copy: {
      dist: "views/main",
    },
    watchIgnore: ["dist/**"],
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
    baseUrl: releaseBaseUrl,
  },
} satisfies ElectrobunConfig
