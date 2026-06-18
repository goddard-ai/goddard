#!/usr/bin/env bun
import { spawnSync } from "node:child_process"
import { existsSync } from "node:fs"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

const workspaceDir = dirname(dirname(fileURLToPath(import.meta.url)))
const executableName = process.platform === "win32" ? "bun.exe" : "bun"
const runtimeBunPath = join(workspaceDir, "node_modules", ".bin", executableName)

if (!existsSync(runtimeBunPath)) {
  console.error(
    `Workspace Bun runtime is not installed at ${runtimeBunPath}. Run pnpm install first.`,
  )
  process.exit(1)
}

const result = spawnSync(runtimeBunPath, process.argv.slice(2), {
  cwd: process.cwd(),
  stdio: "inherit",
  env: process.env,
})

if (result.error) {
  throw result.error
}

process.exit(result.status ?? 1)
