import bunPluginRaw from "bun-plugin-raw"
import type { ElectrobunConfig } from "electrobun"
import pkg from "./package.json" with { type: "json" }
import { svgIconBuildPlugin } from "./src/bun/plugins/svg-icon-build-plugin"

export default {
  app: {
    name: "Goddard",
    identifier: "app.goddardai.org",
    version: pkg.version,
  },
  build: {
    watch: ["styled-system", "public"],
    bun: {
      entrypoint: "src/bun/index.ts",
      plugins: [svgIconBuildPlugin(), bunPluginRaw],
    },
    views: {
      main: {
        entrypoint: "src/main/index.tsx",
        jsx: {
          runtime: "automatic",
          importSource: "preact",
        },
      },
    },
    copy: {
      "src/main/index.html": "views/main/index.html",
      "src/main/index.css": "views/main/index.css",
    },
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
} satisfies ElectrobunConfig
