/// <reference types="vitest/config" />

import { resolve } from "node:path"
import { lingui } from "@lingui/vite-plugin"
import preact from "@preact/preset-vite"
import tsrxPreact from "@tsrx/vite-plugin-preact"
import { sourceSyntax } from "sculpted/tsrx"
import { sculpted } from "sculpted/vite"
import { defineConfig } from "vite"

import svgIcons from "./plugins/svg-icon-build-plugin.ts"

const devServerUrl = process.env.GODDARD_APP_DEV_SERVER_URL
  ? new URL(process.env.GODDARD_APP_DEV_SERVER_URL)
  : undefined

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
      include: /\.[cm]?[jt]sx?$|\.tsrx$/,
      // Workspace source can contain standard decorators; transform them before Prefresh parses HMR signatures.
      babel: {
        plugins: [
          "@lingui/babel-plugin-lingui-macro",
          ["@babel/plugin-proposal-decorators", { version: "2023-11" }],
        ],
        parserOpts: {
          plugins: ["typescript"],
        },
      },
    }),
  ],
  server: {
    host: devServerUrl?.hostname ?? "127.0.0.1",
    port: Number(devServerUrl?.port ?? 5173),
    strictPort: true,
  },
  build: {
    // relative to "src/main"
    outDir: "../../build/views/main",
    emptyOutDir: true,
  },
  optimizeDeps: {
    exclude: ["state-launcher"],
  },
  resolve: {
    alias: {
      "@goddard-ai/ui-primitives": resolve(
        import.meta.dirname,
        "../core/ui-primitives/src/index.ts",
      ),
      "~": resolve(import.meta.dirname, "src"),
    },
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
