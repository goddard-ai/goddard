import { defineConfig } from "tsdown"

const isDebug = process.env.DEBUG === "true"

export default defineConfig({
  entry: ["./src/daemon.ts", "./src/daemon-ipc.ts", "./src/sdk.ts", "./src/schema.ts"],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
  sourcemap: isDebug,
  dts: {
    tsgo: true,
  },
})
