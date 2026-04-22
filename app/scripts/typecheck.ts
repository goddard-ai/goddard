#!/usr/bin/env bun
import { execFileSync, spawnSync } from "node:child_process"
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"

/** Lists the app source files that tsgo needs passed explicitly for `.tsrx`. */
function getTypecheckFiles() {
  const output = execFileSync("rg", ["--files", "src", "plugins", "-g", "*.ts", "-g", "*.tsrx"], {
    encoding: "utf8",
    cwd: path.resolve(import.meta.dir, ".."),
  })

  return output
    .split(/\r?\n/)
    .map((file) => file.trim())
    .filter(Boolean)
}

/** Runs tsgo against a temporary config that enumerates the `.tsrx` inputs. */
function main() {
  const appRoot = path.resolve(import.meta.dir, "..")
  const tempDir = mkdtempSync(path.join(tmpdir(), "goddard-app-tsgo-"))
  const tempConfigPath = path.join(tempDir, "tsconfig.json")
  const config = JSON.parse(readFileSync(path.join(appRoot, "tsconfig.json"), "utf8")) as {
    extends?: string
    exclude?: string[]
    files?: string[]
    include?: string[]
  }

  if (config.extends) {
    config.extends = path.resolve(appRoot, config.extends)
  }

  delete config.include
  config.files = getTypecheckFiles()

  writeFileSync(tempConfigPath, JSON.stringify(config, null, 2))

  try {
    const result = spawnSync(
      path.resolve(appRoot, "../node_modules/.bin/tsgo"),
      ["--noEmit", "-p", tempConfigPath],
      {
        cwd: appRoot,
        stdio: "inherit",
      },
    )

    process.exit(result.status ?? 1)
  } finally {
    rmSync(tempDir, { recursive: true, force: true })
  }
}

main()
