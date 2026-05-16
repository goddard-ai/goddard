#!/usr/bin/env node
import { spawnSync } from "node:child_process"
import { existsSync, readFileSync } from "node:fs"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

const workspaceDir = dirname(dirname(fileURLToPath(import.meta.url)))
const rootPackage = JSON.parse(readFileSync(join(workspaceDir, "package.json"), "utf8"))
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
  fail(`Workspace Bun runtime is not installed at ${runtimeBunPath}. Run bun install first.`)
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

function run(command, args) {
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

function assertVersion(label, actual, expected) {
  if (actual === expected) {
    return
  }

  fail(`${label} is ${actual ?? "unset"}, expected ${expected}`)
}

function fail(message) {
  console.error(message)
  process.exit(1)
}
