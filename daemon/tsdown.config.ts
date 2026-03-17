import { defineConfig } from "tsdown"
import unpluginRaw from "unplugin-raw/rollup"
import { readFileSync } from "node:fs"

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf-8"))

export default defineConfig({
  entry: ["./src/index.ts", "./src/main.ts", "./src/bin/goddard-tool.ts"],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
  dts: true,
  plugins: [unpluginRaw()],
  define: {
    __VERSION__: JSON.stringify(pkg.version),
  },
})
