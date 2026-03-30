import type { ElectrobunConfig } from "electrobun"
import pkg from "./package.json" with { type: "json" }

export default {
  app: {
    name: "Goddard",
    identifier: "app.goddardai.org",
    version: pkg.version,
  },
  build: {
    watch: ["styled-system"],
    bun: {
      entrypoint: "src/bun/index.ts",
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
      icon: "assets/icon.ico",
    },
    linux: {
      icon: "assets/icon.png",
    },
  },
} satisfies ElectrobunConfig
