import preact from "@preact/preset-vite"
import tsrxPreact from "@tsrx/vite-plugin-preact"
import { defineConfig } from "vite"

import svgIcons from "./plugins/svg-icon-build-plugin.ts"

/** Vite config for the desktop webview source rooted at src/main. */
export default defineConfig({
  root: "src/main",
  base: "./",
  publicDir: "../../public",
  plugins: [svgIcons(), tsrxPreact(), preact()],
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
