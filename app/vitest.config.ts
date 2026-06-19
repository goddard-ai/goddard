import { lingui } from "@lingui/vite-plugin"
import preact from "@preact/preset-vite"
import tsrxPreact from "@tsrx/vite-plugin-preact"
import { sourceSyntax } from "sculpted/tsrx"
import { sculpted } from "sculpted/vite"
import { defineConfig } from "vitest/config"

import svgIcons from "./plugins/svg-icon-build-plugin.ts"

export default defineConfig({
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
  resolve: {
    conditions: ["bun"],
    tsconfigPaths: true,
  },
  test: {
    environment: "happy-dom",
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    server: {
      deps: {
        inline: ["powerkeys", "@casbin/expression-eval"],
      },
    },
    setupFiles: ["./test-setup.ts"],
  },
})
