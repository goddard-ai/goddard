#!/usr/bin/env bun
/*
 * Verifies that the workspace Bun runtime and Electrobun build configuration
 * both use the Bun version pinned in the root pnpm workspace catalog.
 */
import { spawnSync } from "node:child_process"
import { existsSync, readFileSync } from "node:fs"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

const workspaceDir = dirname(dirname(fileURLToPath(import.meta.url)))
const expectedVersion = readWorkspaceCatalogBunVersion(
  readFileSync(join(workspaceDir, "pnpm-workspace.yaml"), "utf8"),
)
const runtimeBunPath = join(
  workspaceDir,
  "node_modules",
  ".bin",
  process.platform === "win32" ? "bun.exe" : "bun",
)

if (!expectedVersion) {
  fail("pnpm-workspace.yaml#catalog.bun must pin the daemon runtime Bun version")
}

if (!existsSync(runtimeBunPath)) {
  fail(`Workspace Bun runtime is not installed at ${runtimeBunPath}. Run pnpm install first.`)
}

const installedVersion = run(runtimeBunPath, ["--version"]).trim()
const electrobunVersion = run(runtimeBunPath, [
  "--print",
  "import config from './app/electrobun.config.ts'; console.log(config.build.bunVersion)",
])
  .trim()
  .split(/\r?\n/)
  .find((line) => line && line !== "undefined")

assertVersion("workspace Bun runtime", installedVersion, expectedVersion)
assertVersion("Electrobun build.bunVersion", electrobunVersion, expectedVersion)

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

function readWorkspaceCatalogBunVersion(workspaceYaml: string) {
  const catalogSection = /(?:^|\n)catalog:\n((?:^[ \t].*\n?)*)/m.exec(workspaceYaml)?.[1]
  if (!catalogSection) {
    return undefined
  }

  return /^\s{2}bun:\s*(\S+)\s*$/m.exec(catalogSection)?.[1]
}

function fail(message: string): never {
  console.error(message)
  process.exit(1)
}
