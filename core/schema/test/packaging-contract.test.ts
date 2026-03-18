import { readFileSync } from "node:fs"
import { assert, test } from "vitest"

test("schema package builds runtime JavaScript with tsgo", () => {
  const packageJson = JSON.parse(
    readFileSync(new URL("../package.json", import.meta.url), "utf8"),
  ) as {
    scripts?: { build?: string }
    exports?: Record<string, unknown>
    dependencies?: Record<string, string>
  }
  const tsconfig = JSON.parse(
    readFileSync(new URL("../tsconfig.json", import.meta.url), "utf8"),
  ) as {
    compilerOptions?: { noEmit?: boolean }
  }
  const daemonIpcSource = readFileSync(new URL("../src/daemon-ipc.ts", import.meta.url), "utf8")

  assert.match(packageJson.scripts?.build ?? "", /\btsgo\b/)
  assert.equal(tsconfig.compilerOptions?.noEmit, false)

  const wildcardExport = packageJson.exports?.["./*"] as { import?: string } | undefined
  assert.match(wildcardExport?.import ?? "", /^\.\/dist\/.+\.js$/)

  assert.equal(typeof packageJson.dependencies?.["@goddard-ai/ipc"], "string")
  assert.match(daemonIpcSource, /from "@goddard-ai\/ipc"/)
})
