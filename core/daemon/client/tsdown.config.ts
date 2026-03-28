import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["./src/index.ts", "./src/node/index.ts"],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
  dts: {
    tsgo: true,
  },
})
