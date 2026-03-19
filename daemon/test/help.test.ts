import { test, expect } from "vitest"
import { spawnSync } from "node:child_process"
import { join } from "node:path"

const BIN_PATH = join(__dirname, "../dist/main.mjs")

test("goddard-daemon prints help when --help is passed", () => {
  const result = spawnSync("node", [BIN_PATH, "--help"], { encoding: "utf-8" })
  expect(result.stdout).toContain("goddard-daemon")
  expect(result.stdout).toContain("run")
  expect(result.status).toBe(0)
})

test("goddard-daemon run prints help when --help is passed", () => {
  const result = spawnSync("node", [BIN_PATH, "run", "--help"], { encoding: "utf-8" })
  expect(result.stdout).toContain("run")
  expect(result.stdout).toContain("--project-dir")
  expect(result.stdout).not.toContain("--repo")
  expect(result.stdout).toContain("--pretty")
  expect(result.stdout).toContain("--verbose")
  expect(result.status).toBe(0)
})
