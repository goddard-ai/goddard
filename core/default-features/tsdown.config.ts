import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["./src/daemon.ts", "./src/daemon-ipc.ts"],
  format: "esm",
  target: "node18",
  clean: true,
  outDir: "dist",
  dts: {
    tsgo: true,
  },
  deps: {
    onlyBundle: false,
  },
})
