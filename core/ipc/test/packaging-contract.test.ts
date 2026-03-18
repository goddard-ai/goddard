import { readFileSync } from "node:fs"
import { assert, test } from "vitest"

test("ipc package runtime imports resolve to built dist files", () => {
  const packageJson = JSON.parse(
    readFileSync(new URL("../package.json", import.meta.url), "utf8"),
  ) as {
    exports?: Record<string, unknown>
  }

  const rootExport = packageJson.exports?.["."] as { import?: string; types?: string } | undefined
  const wildcardExport = packageJson.exports?.["./*"] as
    | { import?: string; types?: string }
    | undefined

  assert.match(rootExport?.import ?? "", /^\.\/dist\/.+\.mjs$/)
  assert.match(rootExport?.types ?? "", /^\.\/dist\/.+\.d\.mts$/)
  assert.match(wildcardExport?.import ?? "", /^\.\/dist\/.+\.mjs$/)
  assert.match(wildcardExport?.types ?? "", /^\.\/dist\/.+\.d\.mts$/)
})
