import { defineConfig } from "tsdown"
import unpluginRaw from "unplugin-raw/rollup"

export default defineConfig({
  entry: ["./src/index.ts", "./src/daemon/index.ts", "./src/loop/index.ts", "./src/node/index.ts"],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
  dts: {
    eager: true,
  },
  plugins: [unpluginRaw()],
  deps: {
    onlyAllowBundle: false,
  },
})
