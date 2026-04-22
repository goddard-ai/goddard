import preact from "@preact/preset-vite"
import tsrxReact from "@tsrx/vite-plugin-react"
import { defineConfig } from "vite"

import svgIcons from "./plugins/svg-icon-build-plugin.ts"

/** Vite config for the desktop webview source rooted at src/main. */
export default defineConfig({
  root: "src/main",
  base: "./",
  publicDir: "../../public",
  plugins: [svgIcons(), tsrxReact({ jsxImportSource: "preact" }), preact()],
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: "../../dist",
    emptyOutDir: true,
  },
  resolve: {
    conditions: ["bun"],
    tsconfigPaths: true,
  },
})
