import { execFileSync } from "node:child_process"

function run(label, command, args) {
  process.stdout.write(`\n[smoke:workforce] ${label}\n`)
  execFileSync(command, args, {
    stdio: "inherit",
    cwd: process.cwd(),
  })
}

run("TypeScript: core/sdk workforce surface", "bun", [
  "exec",
  "tsgo",
  "-p",
  "core/sdk/tsconfig.json",
  "--noEmit",
])

run("TypeScript: workforce CLI", "bun", [
  "exec",
  "tsgo",
  "-p",
  "workforce/tsconfig.json",
  "--noEmit",
])

run("Vitest: workforce smoke suites", "bun", [
  "exec",
  "vitest",
  "run",
  "core/sdk/test/workforce.test.ts",
  "core/daemon/test/workforce.test.ts",
  "workforce/test/main.test.ts",
])

process.stdout.write("\n[smoke:workforce] complete\n")
