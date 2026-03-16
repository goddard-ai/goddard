import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["./src/index.ts", "./src/client.ts", "./src/server.ts"],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
  dts: true,
})
