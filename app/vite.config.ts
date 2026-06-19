/// <reference types="vitest/config" />

import { lingui } from "@lingui/vite-plugin"
import preact from "@preact/preset-vite"
import tsrxPreact from "@tsrx/vite-plugin-preact"
import { sourceSyntax } from "sculpted/tsrx"
import { sculpted } from "sculpted/vite"
import { defineConfig } from "vite"

import svgIcons from "./plugins/svg-icon-build-plugin.ts"

/** Vite config for the desktop webview source rooted at src/main. */
export default defineConfig({
  root: "src/main",
  base: "./",
  publicDir: "../../public",
  plugins: [
    lingui(),
    svgIcons(),
    sculpted({
      projectRoot: import.meta.dirname,
      include: ["src/**/*.{ts,tsx,tsrx}"],
      sourceSyntax,
      panda: {
        cssImportSources: ["@goddard-ai/styled-system/css"],
      },
    }),
    tsrxPreact(),
    preact({
      // Workspace source can contain standard decorators; transform them before Prefresh parses HMR signatures.
      babel: {
        plugins: [["@babel/plugin-proposal-decorators", { version: "2023-11" }]],
      },
    }),
  ],
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
  },
  build: {
    // relative to "src/main"
    outDir: "../../build/views/main",
    emptyOutDir: true,
  },
  resolve: {
    conditions: ["bun"],
    tsconfigPaths: true,
  },
  test: {
    environment: "happy-dom",
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    root: import.meta.dirname,
    server: {
      deps: {
        inline: ["powerkeys", "@casbin/expression-eval"],
      },
    },
    setupFiles: ["./test-setup.ts"],
  },
})
