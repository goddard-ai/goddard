import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["./src/index.ts"],
  format: "esm",
  target: "es2020",
  clean: true,
  outDir: "dist",
  dts: true,
})
