#!/usr/bin/env bun
/*
 * Verifies that the workspace Bun runtime and Electrobun build configuration
 * both use the Bun version pinned in the root package catalog.
 */
import { spawnSync } from "node:child_process"
import { existsSync, readFileSync } from "node:fs"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

type RootPackage = {
  catalog?: {
    bun?: string
  }
}

const workspaceDir = dirname(dirname(fileURLToPath(import.meta.url)))
const rootPackage = JSON.parse(
  readFileSync(join(workspaceDir, "package.json"), "utf8"),
) as RootPackage
const expectedVersion = rootPackage.catalog?.bun
const runtimeBunPath = join(
  workspaceDir,
  "node_modules",
  ".bin",
  process.platform === "win32" ? "bun.exe" : "bun",
)

if (!expectedVersion) {
  fail("package.json#catalog.bun must pin the daemon runtime Bun version")
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

function fail(message: string): never {
  console.error(message)
  process.exit(1)
}
