import { defineConfig } from "tsdown"

const isDebug = process.env.DEBUG === "true"

export default defineConfig({
  entry: ["./src/index.ts", "./src/testing.ts"],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
  sourcemap: isDebug,
  deps: {
    neverBundle: ["bun:ffi"],
  },
  dts: {
    tsgo: true,
  },
})
