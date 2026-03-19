import { readFileSync } from "node:fs"
import { defineConfig } from "tsdown"
import unpluginRaw from "unplugin-raw/rollup"

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf-8"))
const isDebug = process.env.DEBUG === "true"

export default defineConfig({
  entry: ["./src/index.ts", "./src/main.ts", "./src/bin/*.ts"],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
  sourcemap: isDebug,
  dts: true,
  plugins: [unpluginRaw()],
  define: {
    __VERSION__: JSON.stringify(pkg.version),
  },
})
