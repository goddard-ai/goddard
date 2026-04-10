import { execFileSync } from "node:child_process"

function run(label, command, args, cwd = process.cwd()) {
  process.stdout.write(`\n[smoke:workforce] ${label}\n`)
  execFileSync(command, args, {
    stdio: "inherit",
    cwd,
  })
}

run("TypeScript: core/sdk workforce surface", "bun", ["run", "--cwd", "core/sdk", "typecheck"])

run("TypeScript: workforce CLI", "bun", ["run", "--cwd", "workforce", "typecheck"])

run(
  "Bun: daemon workforce suite",
  "bun",
  ["test", "--dots", "test/workforce.test.ts"],
  "core/daemon",
)

process.stdout.write("\n[smoke:workforce] complete\n")
