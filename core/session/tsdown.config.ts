import { defineConfig } from "tsdown"
import unpluginRaw from "unplugin-raw/rollup"

export default defineConfig({
  entry: ["./src/**/*.ts"],
  unbundle: true,
  format: "esm",
  target: "node20",
  clean: true,
  outDir: "dist",
  dts: {
    eager: true,
  },
  plugins: [unpluginRaw()],
})
