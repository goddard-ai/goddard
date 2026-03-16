import { defineConfig } from "tsdown"
import unpluginRaw from "unplugin-raw/rollup"
import { readFileSync } from "node:fs"

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf-8"))

export default defineConfig({
  entry: ["./src/**/*.ts"],
  format: "esm",
  target: "node20",
  clean: true,
  outDir: "dist",
  define: {
    __VERSION__: JSON.stringify(pkg.version),
  },
  dts: {
    eager: true,
  },
  plugins: [unpluginRaw()],
})
