import { readFile } from "node:fs/promises"
import { assert, test } from "vitest"

test("daemon package does not depend on @goddard-ai/sdk", async () => {
  const packageJson = JSON.parse(
    await readFile(new URL("../package.json", import.meta.url), "utf-8"),
  ) as {
    dependencies?: Record<string, string>
  }

  assert.equal(packageJson.dependencies?.["@goddard-ai/sdk"], undefined)
})
