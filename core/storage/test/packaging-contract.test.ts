import { readFileSync } from "node:fs"
import { assert, test } from "vitest"

test("storage package builds runtime JavaScript with tsgo", () => {
  const packageJson = JSON.parse(
    readFileSync(new URL("../package.json", import.meta.url), "utf8"),
  ) as {
    scripts?: { build?: string }
    exports?: Record<string, unknown>
  }
  const tsconfig = JSON.parse(
    readFileSync(new URL("../tsconfig.json", import.meta.url), "utf8"),
  ) as {
    compilerOptions?: { noEmit?: boolean }
  }

  assert.match(packageJson.scripts?.build ?? "", /\btsgo\b/)
  assert.equal(tsconfig.compilerOptions?.noEmit, false)

  const rootExport = packageJson.exports?.["."] as { import?: string } | undefined
  const sessionPermissionsExport = packageJson.exports?.["./session-permissions"] as
    | { import?: string }
    | undefined

  assert.match(rootExport?.import ?? "", /^\.\/dist\/.+\.js$/)
  assert.match(sessionPermissionsExport?.import ?? "", /^\.\/dist\/.+\.js$/)
})
