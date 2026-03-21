import { test, expect } from "bun:test"
import { spawnSync } from "node:child_process"
import { join } from "node:path"

const BIN_PATH = join(__dirname, "../src/main.ts")

test("goddard-daemon prints help when --help is passed", () => {
  const result = spawnSync("bun", ["--conditions", "source", BIN_PATH, "--help"], {
    encoding: "utf-8",
  })
  const output = `${result.stdout}${result.stderr}`
  expect(output).toContain("goddard-daemon")
  expect(output).toContain("run")
  expect(result.status).toBe(0)
})

test("goddard-daemon run prints help when --help is passed", () => {
  const result = spawnSync("bun", ["--conditions", "source", BIN_PATH, "run", "--help"], {
    encoding: "utf-8",
  })
  const output = `${result.stdout}${result.stderr}`
  expect(output).toContain("run")
  expect(output).toContain("--base-url")
  expect(output).not.toContain("--repo")
  expect(output).not.toContain("--project-dir")
  expect(output).toContain("--json")
  expect(output).toContain("--verbose")
  expect(result.status).toBe(0)
})
