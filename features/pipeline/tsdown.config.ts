import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["./src/schema.ts", "./src/loader.ts", "./src/daemon.ts", "./src/daemon-ipc.ts"],
  format: ["esm"],
  dts: true,
  tsconfig: "./tsconfig.json",
  tsgo: true,
})
