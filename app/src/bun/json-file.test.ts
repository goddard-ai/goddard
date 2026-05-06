import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"
import { z } from "zod"

import { readJsonFile, writeJsonFile } from "./json-file.ts"

const JsonFixture = z.strictObject({
  version: z.literal(1),
  value: z.string(),
})

const tempDirs: string[] = []

afterEach(async () => {
  await Promise.all(
    tempDirs.splice(0).map((directory) => rm(directory, { force: true, recursive: true })),
  )
})

test("readJsonFile returns null when the file is absent", async () => {
  const filePath = join(await createTempDir(), "nested", "settings.json")

  await expect(readJsonFile(filePath, JsonFixture)).resolves.toBeNull()
})

test("readJsonFile validates the parsed JSON file", async () => {
  const filePath = join(await createTempDir(), "settings.json")
  await writeFile(filePath, JSON.stringify({ version: 1, value: 2 }), "utf8")

  await expect(readJsonFile(filePath, JsonFixture)).rejects.toThrow()
})

test("writeJsonFile creates parent directories and writes pretty JSON", async () => {
  const filePath = join(await createTempDir(), "nested", "settings.json")
  const value = {
    version: 1 as const,
    value: "saved",
  }

  await writeJsonFile(filePath, value)

  await expect(readJsonFile(filePath, JsonFixture)).resolves.toEqual(value)
  await expect(readFile(filePath, "utf8")).resolves.toBe(`${JSON.stringify(value, null, 2)}\n`)
})

async function createTempDir() {
  const directory = await mkdtemp(join(tmpdir(), "goddard-json-file-"))
  tempDirs.push(directory)
  return directory
}
