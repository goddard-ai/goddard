import { defineConfig } from "tsdown"

const isDebug = process.env.DEBUG === "true"

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
  sourcemap: isDebug,
  dts: {
    tsgo: true,
  },
})
