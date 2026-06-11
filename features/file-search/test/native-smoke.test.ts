import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

const tempDirectories: string[] = []

afterEach(async () => {
  await Promise.all(
    tempDirectories.splice(0).map((path) => rm(path, { recursive: true, force: true })),
  )
})

async function createTempProject() {
  const directory = await mkdtemp(join(tmpdir(), "goddard-fff-native-"))
  tempDirectories.push(directory)
  return directory
}

describe("fff native file search", () => {
  test("loads the native package and creates a finder", async () => {
    const cwd = await createTempProject()
    await writeFile(join(cwd, "native-smoke.ts"), "export const smoke = true\n")
    const { FileFinder } = await import("@ff-labs/fff-bun")
    const created = FileFinder.create({
      basePath: cwd,
      aiMode: true,
      disableContentIndexing: true,
    })

    expect(created.ok).toBe(true)

    if (!created.ok) {
      throw new Error(created.error)
    }

    try {
      const scan = created.value.waitForScan(5_000)
      expect(scan.ok).toBe(true)
    } finally {
      created.value.destroy()
    }
  })
})
