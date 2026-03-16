import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { fileURLToPath } from "node:url"
import { describe, expect, test } from "vitest"

const testDir = fileURLToPath(new URL(".", import.meta.url))
const packageJsonPath = join(testDir, "..", "package.json")

describe("schema package exports", () => {
  test("uses wildcard submodule exports without a root export", async () => {
    const packageJson = JSON.parse(await readFile(packageJsonPath, "utf8")) as {
      exports?: Record<string, string>
    }

    expect(packageJson.exports).toEqual({
      "./*": "./src/*.ts",
    })
  })
})
