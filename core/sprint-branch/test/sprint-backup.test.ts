import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import { sprintBackupPath } from "../src/sprint-backup"
import { cleanupTestRepos, createBaseRepo, pathExists, runCli } from "./support"

type RestoreOutput = {
  ok: boolean
  dryRun: boolean
  executed: boolean
  backedUpAt: string | null
  diagnostics: Array<{ code: string }>
}

describe("sprint-branch sprint folder backups", () => {
  afterEach(cleanupTestRepos)

  test("backs up sprint tasks when state is written", async () => {
    const repo = await createBaseRepo("example")

    const result = await runCli(repo, ["init", "--sprint", "example", "--base", "main", "--json"])

    expect(result.exitCode).toBe(0)
    await expect(
      pathExists(path.join(await sprintBackupPath(repo, "example"), "files", "010-task-name.md")),
    ).resolves.toBe(true)
    await expect(
      pathExists(path.join(await sprintBackupPath(repo, "example"), "manifest.json")),
    ).resolves.toBe(true)
  })

  test("refreshes the latest backup from the live sprint folder", async () => {
    const repo = await createBaseRepo("example")
    await runCli(repo, ["init", "--sprint", "example", "--base", "main", "--json"])
    await fs.writeFile(
      path.join(repo, "sprints", "example", "030-added-after-init.md"),
      "# added after init\n",
    )

    const result = await runCli(repo, ["reset-state", "--sprint", "example", "--json"])

    expect(result.exitCode).toBe(0)
    await expect(
      pathExists(
        path.join(await sprintBackupPath(repo, "example"), "files", "030-added-after-init.md"),
      ),
    ).resolves.toBe(true)
  })

  test("restores a missing sprint folder from the latest backup", async () => {
    const repo = await createBaseRepo("example")
    await runCli(repo, ["init", "--sprint", "example", "--base", "main", "--json"])
    await fs.rm(path.join(repo, "sprints", "example"), { recursive: true, force: true })

    const result = await runCli(repo, ["restore-sprint", "--sprint", "example", "--json"])
    const restore = JSON.parse(result.stdout) as RestoreOutput

    expect(result.exitCode).toBe(0)
    expect(restore.ok).toBe(true)
    expect(restore.executed).toBe(true)
    expect(restore.backedUpAt).not.toBeNull()
    await expect(
      pathExists(path.join(repo, "sprints", "example", "010-task-name.md")),
    ).resolves.toBe(true)
  })

  test("refuses to overwrite an existing sprint folder without force", async () => {
    const repo = await createBaseRepo("example")
    await runCli(repo, ["init", "--sprint", "example", "--base", "main", "--json"])

    const result = await runCli(repo, ["restore-sprint", "--sprint", "example", "--json"])
    const restore = JSON.parse(result.stdout) as RestoreOutput

    expect(result.exitCode).toBe(1)
    expect(restore.ok).toBe(false)
    expect(restore.executed).toBe(false)
    expect(restore.diagnostics.map((diagnostic) => diagnostic.code)).toContain(
      "sprint_folder_exists",
    )
  })
})
