import { readFile } from "node:fs/promises"
import { expect, test } from "vitest"

test("daemon package does not depend on removed higher-level packages", async () => {
  const packageJson = JSON.parse(
    await readFile(new URL("../package.json", import.meta.url), "utf-8"),
  ) as {
    dependencies?: Record<string, string>
  }

  expect(packageJson.dependencies?.["@goddard-ai/sdk"]).toBeUndefined()
  expect(packageJson.dependencies?.["@goddard-ai/backend-client"]).toBeUndefined()
})
