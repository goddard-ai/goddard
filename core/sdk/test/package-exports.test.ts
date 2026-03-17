import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { fileURLToPath } from "node:url"
import { describe, expect, test } from "vitest"

const testDir = fileURLToPath(new URL(".", import.meta.url))
const packageJsonPath = join(testDir, "..", "package.json")

describe("sdk package exports", () => {
  test("publishes root, daemon, loop, and node entry points only", async () => {
    const packageJson = JSON.parse(await readFile(packageJsonPath, "utf8")) as {
      exports?: Record<string, unknown>
    }

    expect(packageJson.exports).toEqual({
      ".": {
        types: "./dist/index.d.mts",
        import: "./dist/index.mjs",
        default: "./src/index.ts",
      },
      "./daemon": {
        types: "./dist/daemon/index.d.mts",
        import: "./dist/daemon/index.mjs",
        default: "./src/daemon/index.ts",
      },
      "./loop": {
        types: "./dist/loop/index.d.mts",
        import: "./dist/loop/index.mjs",
        default: "./src/loop/index.ts",
      },
      "./node": {
        types: "./dist/node/index.d.mts",
        import: "./dist/node/index.mjs",
        default: "./src/node/index.ts",
      },
    })
  })

  test("keeps transport helpers out of sdk/daemon ownership", async () => {
    const rootExports = await import("../src/index.ts")
    const daemonExports = await import("../src/daemon/index.ts")
    const loopExports = await import("../src/loop/index.ts")
    const nodeExports = await import("../src/node/index.ts")

    expect(rootExports).toHaveProperty("GoddardSdk")
    expect(rootExports).not.toHaveProperty("runAgent")

    expect(daemonExports).toHaveProperty("AgentSession")
    expect(daemonExports).toHaveProperty("runAgent")
    expect(daemonExports).not.toHaveProperty("createDaemonIpcClient")

    expect(loopExports).toHaveProperty("runAgentLoop")
    expect(loopExports).not.toHaveProperty("createDaemonIpcClient")

    expect(nodeExports).toHaveProperty("GoddardSdk")
    expect(nodeExports).toHaveProperty("FileTokenStorage")
    expect(nodeExports).not.toHaveProperty("createDaemonIpcClient")
  })
})
