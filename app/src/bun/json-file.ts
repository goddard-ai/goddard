import { randomUUID } from "node:crypto"
import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises"
import { dirname } from "node:path"

function isNodeErrorCode(error: unknown, code: string) {
  return error instanceof Error && "code" in error && error.code === code
}

/** Reads and validates an optional JSON file, returning null when the file is absent. */
export async function readJsonFile<T>(filePath: string, schema: { parse(value: unknown): T }) {
  let source: string

  try {
    source = await readFile(filePath, "utf8")
  } catch (error) {
    if (isNodeErrorCode(error, "ENOENT")) {
      return null
    }

    throw error
  }

  return schema.parse(JSON.parse(source))
}

/** Atomically writes one pretty-printed JSON file with a trailing newline. */
export async function writeJsonFile(filePath: string, value: unknown) {
  const temporaryPath = `${filePath}.${process.pid}.${randomUUID()}.tmp`

  await mkdir(dirname(filePath), { recursive: true })

  try {
    await writeFile(temporaryPath, `${JSON.stringify(value, null, 2)}\n`, "utf8")
    await rename(temporaryPath, filePath)
  } catch (error) {
    await rm(temporaryPath, { force: true }).catch(() => {})
    throw error
  }
}
