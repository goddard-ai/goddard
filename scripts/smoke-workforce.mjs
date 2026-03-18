import { execFileSync } from "node:child_process"

function run(label, command, args) {
  process.stdout.write(`\n[smoke:workforce] ${label}\n`)
  execFileSync(command, args, {
    stdio: "inherit",
    cwd: process.cwd(),
  })
}

run("TypeScript: core/sdk workforce surface", "pnpm", [
  "exec",
  "tsc",
  "-p",
  "core/sdk/tsconfig.json",
  "--noEmit",
])

run("TypeScript: workforce CLI", "pnpm", [
  "exec",
  "tsc",
  "-p",
  "workforce/tsconfig.json",
  "--noEmit",
])

run("Vitest: workforce smoke suites", "pnpm", [
  "exec",
  "vitest",
  "run",
  "core/sdk/test/workforce.test.ts",
  "daemon/test/workforce.test.ts",
  "workforce/test/main.test.ts",
])

process.stdout.write("\n[smoke:workforce] complete\n")
