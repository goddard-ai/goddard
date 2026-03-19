import { defineConfig } from "tsdown"
import unpluginRaw from "unplugin-raw/rollup"

const isDebug = process.env.DEBUG === "true"

export default defineConfig({
  entry: ["./src/index.ts", "./src/daemon/index.ts", "./src/loop/index.ts", "./src/node/index.ts"],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
  sourcemap: isDebug,
  dts: {
    tsgo: true,
  },
  plugins: [unpluginRaw()],
  deps: {
    onlyAllowBundle: false,
  },
})
