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
        entrypoint: "src/main.tsx",
        jsx: {
          runtime: "automatic",
          importSource: "preact",
        },
      },
    },
    copy: {
      "index.html": "views/main/index.html",
    },
  },
} satisfies ElectrobunConfig
