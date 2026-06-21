import { dirname } from "node:path"

import { ensureDir, pathExists, readVersions, rootDir, run, sourceDir } from "./common.ts"

const versions = await readVersions()

await ensureDir(dirname(sourceDir))

if (!(await pathExists(sourceDir))) {
  await run("git", [
    "clone",
    "--depth",
    "1",
    "--branch",
    versions.libgit2.ref,
    versions.libgit2.repo,
    sourceDir,
  ])
} else {
  await run("git", ["fetch", "--depth", "1", "origin", versions.libgit2.ref], { cwd: sourceDir })
  await run("git", ["checkout", "FETCH_HEAD"], { cwd: sourceDir })
}

const { stdout } = await run("git", ["rev-parse", "HEAD"], { cwd: sourceDir })
console.log(`Fetched libgit2 ${versions.libgit2.ref} at ${stdout.trim()} in ${sourceDir}`)
console.log(`Native pipeline root: ${rootDir}`)
