import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["./src/schema.ts"],
  format: ["esm"],
  dts: true,
  tsconfig: "./tsconfig.json",
  tsgo: true,
})
