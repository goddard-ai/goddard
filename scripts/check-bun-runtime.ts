#!/usr/bin/env bun
/*
 * Verifies that the installed workspace Bun runtime matches the monorepo pin.
 */
import { spawnSync } from "node:child_process"
import { existsSync, readFileSync } from "node:fs"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

type WorkspaceConfig = {
  catalog?: {
    bun?: string
  }
}

const workspaceDir = dirname(dirname(fileURLToPath(import.meta.url)))
const bunVersion = readFileSync(join(workspaceDir, ".bun-version"), "utf8").trim()
const workspaceConfig = Bun.YAML.parse(
  readFileSync(join(workspaceDir, "pnpm-workspace.yaml"), "utf8"),
) as WorkspaceConfig
const catalogVersion = workspaceConfig.catalog?.bun
const runtimeBunPath = join(
  workspaceDir,
  "node_modules",
  ".bin",
  process.platform === "win32" ? "bun.exe" : "bun",
)

if (!bunVersion) {
  fail(".bun-version must pin the monorepo Bun version")
}

if (!catalogVersion) {
  fail("pnpm-workspace.yaml#catalog.bun must pin the workspace Bun package version")
}

if (!existsSync(runtimeBunPath)) {
  fail(`Workspace Bun runtime is not installed at ${runtimeBunPath}. Run pnpm install first.`)
}

const installedVersion = run(runtimeBunPath, ["--version"]).trim()

assertVersion("pnpm-workspace.yaml catalog.bun", catalogVersion, bunVersion)
assertVersion("workspace Bun runtime", installedVersion, bunVersion)

function run(command: string, args: string[]) {
  const result = spawnSync(command, args, {
    cwd: workspaceDir,
    encoding: "utf8",
  })

  if (result.error) {
    throw result.error
  }

  if (result.status !== 0) {
    const output = [result.stdout.trim(), result.stderr.trim()].filter(Boolean).join("\n")
    fail(output || `${command} ${args.join(" ")} failed`)
  }

  return result.stdout
}

function assertVersion(label: string, actual: string | undefined, expected: string) {
  if (actual === expected) {
    return
  }

  fail(`${label} is ${actual ?? "unset"}, expected ${expected}`)
}

function fail(message: string): never {
  console.error(message)
  process.exit(1)
}
