import { defineConfig } from "tsdown"

export default defineConfig({
  entry: [
    "./src/index.ts",
    "./src/client.ts",
    "./src/node-client.ts",
    "./src/schema.ts",
    "./src/server.ts",
    "./src/transport.ts",
  ],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
  dts: true,
})
