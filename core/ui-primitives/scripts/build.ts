import { rm } from "node:fs/promises"
import { tsrxPreact } from "@tsrx/bun-plugin-preact"

await rm("dist", {
  force: true,
  recursive: true,
})

const result = await Bun.build({
  entrypoints: ["src/index.ts"],
  external: ["@floating-ui/dom", "@preact/signals", "preact", "preact/*", "tabbable"],
  format: "esm",
  naming: {
    entry: "[dir]/[name].mjs",
  },
  outdir: "dist",
  plugins: [
    tsrxPreact({
      emitCss: false,
    }),
  ],
  target: "browser",
})

if (!result.success) {
  for (const log of result.logs) {
    console.error(log)
  }

  process.exit(1)
}

await Bun.$`tsrx-tsc -p tsconfig.build.json`
