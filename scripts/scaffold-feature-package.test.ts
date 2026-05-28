import { readFile, rm } from "node:fs/promises"
import { join } from "node:path"
import { describe, expect, test } from "bun:test"

import {
  createFeatureScaffoldPlan,
  createTempScaffoldRoot,
  normalizeFeatureName,
  parseScaffoldArgs,
  writeFeatureScaffoldPlan,
} from "./scaffold-feature-package.ts"

describe("normalizeFeatureName", () => {
  test("normalizes common human feature names to package segments", () => {
    expect(normalizeFeatureName("@goddard-ai/Session Inbox")).toBe("session-inbox")
  })
})

describe("parseScaffoldArgs", () => {
  test("parses noninteractive scaffold options", async () => {
    expect(
      await parseScaffoldArgs([
        "--name",
        "inbox",
        "--layers",
        "daemon,sdk,app",
        "--schema",
        "--daemon-ipc",
        "--dry-run",
        "--skip-install",
      ]),
    ).toEqual({
      name: "inbox",
      layers: ["daemon", "sdk", "app"],
      includeSchema: true,
      includeDaemonIpc: true,
      dryRun: true,
      skipInstall: true,
    })
  })
})

describe("createFeatureScaffoldPlan", () => {
  test("creates SDK and daemon IPC files only when their layers need them", () => {
    const rootDir = join("/", "repo")
    const plan = createFeatureScaffoldPlan({
      name: "inbox",
      rootDir,
      layers: ["daemon", "sdk", "app"],
      includeDaemonIpc: true,
      includeStyledSystem: false,
    })

    expect(plan.files.map((file) => file.path)).toEqual([
      join(rootDir, "features", "inbox", "package.json"),
      join(rootDir, "features", "inbox", "tsconfig.json"),
      join(rootDir, "features", "inbox", "test", "tsconfig.json"),
      join(rootDir, "features", "inbox", "tsdown.config.ts"),
      join(rootDir, "features", "inbox", "test", "feature.test.ts"),
      join(rootDir, "features", "inbox", "src", "app.tsx"),
      join(rootDir, "features", "inbox", "src", "daemon.ts"),
      join(rootDir, "features", "inbox", "src", "daemon-ipc.ts"),
      join(rootDir, "features", "inbox", "src", "sdk.ts"),
    ])

    const packageJson = JSON.parse(plan.files[0]!.content)
    expect(packageJson.dependencies).toEqual({
      "@goddard-ai/daemon-plugin": "workspace:*",
      "@goddard-ai/ipc": "workspace:*",
      "@goddard-ai/sdk-plugin": "workspace:*",
    })
    expect(packageJson.dependencies).not.toHaveProperty("@goddard-ai/styled-system")
  })

  test("adds app style dependencies only when generated app styles need them", () => {
    const plan = createFeatureScaffoldPlan({
      name: "project-activity",
      rootDir: "/repo",
      layers: ["app"],
      includeStyledSystem: true,
    })

    const packageJson = JSON.parse(plan.files[0]!.content)
    expect(packageJson.dependencies).toEqual({
      "@goddard-ai/styled-system": "workspace:*",
    })
    expect(plan.files.some((file) => file.path.endsWith("src/app.style.ts"))).toBe(true)
  })
})

describe("writeFeatureScaffoldPlan", () => {
  test("writes a new inert feature package", async () => {
    const rootDir = await createTempScaffoldRoot()

    try {
      const plan = createFeatureScaffoldPlan({
        name: "inbox",
        rootDir,
        layers: ["daemon", "sdk"],
        includeDaemonIpc: true,
        includeSchema: true,
      })

      await writeFeatureScaffoldPlan(plan)

      await expect(
        readFile(join(rootDir, "features/inbox/src/daemon-ipc.ts"), "utf8"),
      ).resolves.toContain("defineIpcRoutes")
    } finally {
      await rm(rootDir, { recursive: true, force: true })
    }
  })
})
