import { readdir, readFile } from "node:fs/promises"
import { relative, resolve } from "node:path"
import { expect, test } from "bun:test"

const repoRoot = resolve(import.meta.dirname, "../../..")
const scannedRoots = ["core/daemon/src", "features", "workforce/src"]
const allowedMatches = new Set(["workforce/src/main.ts"])
const directGitSubprocessPatterns = [
  /Bun\.spawn(?:Sync)?\(\s*\[\s*["']git["']/,
  /runCommand\(\s*["']git["']/,
  /execFileAsync\(\s*["']git["']/,
  /spawn(?:Sync)?\(\s*["']git["']/,
]

test("daemon-owned production paths do not shell out directly to Git", async () => {
  const matches: string[] = []

  for (const root of scannedRoots) {
    for (const file of await listTypeScriptFiles(resolve(repoRoot, root))) {
      const relativePath = toPosixPath(relative(repoRoot, file))
      if (allowedMatches.has(relativePath) || isTestOrScriptPath(relativePath)) {
        continue
      }

      const source = await readFile(file, "utf8")
      if (directGitSubprocessPatterns.some((pattern) => pattern.test(source))) {
        matches.push(relativePath)
      }
    }
  }

  expect(matches).toEqual([])
})

async function listTypeScriptFiles(root: string): Promise<string[]> {
  const entries = await readdir(root, { withFileTypes: true })
  const files = await Promise.all(
    entries.map(async (entry) => {
      const entryPath = resolve(root, entry.name)
      if (entry.isDirectory()) {
        return await listTypeScriptFiles(entryPath)
      }
      return entry.isFile() && entry.name.endsWith(".ts") ? [entryPath] : []
    }),
  )

  return files.flat()
}

function isTestOrScriptPath(path: string) {
  return path.includes("/test/") || path.includes("/scripts/")
}

function toPosixPath(path: string) {
  return path.replaceAll("\\", "/")
}
