import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["./src/pipeline.ts", "./src/weaver.ts"],
  format: ["esm"],
  dts: true,
  tsconfig: "./tsconfig.json",
  tsgo: true,
})
