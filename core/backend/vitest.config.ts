import path from "node:path"
import { fileURLToPath } from "node:url"
import { cloudflareTest } from "@cloudflare/vitest-pool-workers"
import { defineConfig } from "vitest/config"

const rootDir = path.dirname(fileURLToPath(import.meta.url))
const schemaSrcDir = path.resolve(rootDir, "../schema/src")

export default defineConfig({
  plugins: [
    cloudflareTest({
      additionalExports: {
        CloudSession: "DurableObject",
        UserStream: "DurableObject",
      },
      main: "./src/worker.ts",
      wrangler: { configPath: "./wrangler.toml" },
    }),
  ],
  resolve: {
    alias: [{ find: /^@goddard-ai\/schema\/(.+)$/, replacement: `${schemaSrcDir}/$1.ts` }],
  },
  test: {
    include: ["test/**/*.worker.ts"],
  },
})
