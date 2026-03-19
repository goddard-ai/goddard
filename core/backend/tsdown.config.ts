import { defineConfig } from "tsdown"

const isDebug = process.env.DEBUG === "true"

export default defineConfig({
  entry: ["./src/index.ts", "./src/worker.ts", "./src/client.ts", "./src/server.ts"],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
  sourcemap: isDebug,
  dts: {
    tsgo: true,
  },
})
