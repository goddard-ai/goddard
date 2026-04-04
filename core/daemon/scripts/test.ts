import { mkdtemp, readdir, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
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
async function runTestFile(fileName: string) {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-daemon-test-home-"))

  try {
    return Bun.spawnSync(["bun", "test", "--conditions", "source", join("test", fileName)], {
      cwd: packageDir,
      stdin: "inherit",
      stdout: "inherit",
      stderr: "inherit",
      env: {
        ...process.env,
        HOME: homeDir,
      },
    })
  } finally {
    await rm(homeDir, { recursive: true, force: true })
  }
}

const files = await listTestFiles()

for (const fileName of files) {
  const result = await runTestFile(fileName)
  if (result.exitCode === 0) {
    continue
  }

  process.exit(result.exitCode ?? 1)
}
