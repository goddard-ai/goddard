import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["./src/server.ts"],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
})
