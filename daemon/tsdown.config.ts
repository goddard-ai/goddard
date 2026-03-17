import { defineConfig } from "tsdown"
import unpluginRaw from "unplugin-raw/rollup"

export default defineConfig({
  entry: ["./src/index.ts", "./src/main.ts", "./src/bin/goddard-tool.ts"],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
  dts: true,
  plugins: [unpluginRaw()],
})
