import { readdir } from "node:fs/promises"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

const scriptDir = dirname(fileURLToPath(import.meta.url))
const packageDir = dirname(scriptDir)
const testDir = join(packageDir, "test")

/** Returns the daemon test files in a stable order for isolated Bun runs. */
async function listTestFiles() {
  return (await readdir(testDir))
    .filter((entry) => entry.endsWith(".test.ts"))
    .sort((left, right) => left.localeCompare(right))
}

/** Runs one daemon test file in its own Bun test process to avoid mock leakage. */
function runTestFile(fileName: string) {
  return Bun.spawnSync(["bun", "test", "--conditions", "source", join("test", fileName)], {
    cwd: packageDir,
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
    env: process.env,
  })
}

const files = await listTestFiles()

for (const fileName of files) {
  const result = runTestFile(fileName)
  if (result.exitCode === 0) {
    continue
  }

  process.exit(result.exitCode ?? 1)
}
