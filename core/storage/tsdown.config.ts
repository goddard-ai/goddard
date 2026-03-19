import { defineConfig } from "tsdown"

const isDebug = process.env.DEBUG === "true"

export default defineConfig({
  entry: ["./src/index.ts", "./src/session-permissions.ts", "./src/managed-pr-locations.ts"],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
  sourcemap: isDebug,
  dts: {
    tsgo: true,
  },
})
