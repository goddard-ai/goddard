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
    win: {
      icon: "icon.windows.png",
    },
    linux: {
      icon: "icon.linux.png",
    },
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
  },
} satisfies ElectrobunConfig
