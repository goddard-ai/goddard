import { dlopen, FFIType } from "bun:ffi"

import {
  artifactManifestPath,
  artifactPath,
  assertFile,
  parseOptions,
  printResult,
  resolveTarget,
  run,
  sha256,
} from "./common.ts"

const options = parseOptions()
const targetConfig = await resolveTarget(options.target)
const libraryPath = await artifactPath(options.target)
const manifestPath = await artifactManifestPath(options.target)

await assertFile(libraryPath)
await assertFile(manifestPath)

const fileInfo = await run("file", [libraryPath])
if (!fileInfo.stdout.toLowerCase().includes(targetConfig.arch.toLowerCase())) {
  throw new Error(
    `Artifact ${libraryPath} does not appear to contain architecture ${targetConfig.arch}: ${fileInfo.stdout.trim()}`,
  )
}

if (targetConfig.platform === "darwin") {
  await run("otool", ["-L", libraryPath])
}

const libgit2 = dlopen(libraryPath, {
  git_libgit2_init: {
    args: [],
    returns: FFIType.i32,
  },
})

const initStatus = libgit2.symbols.git_libgit2_init()
if (initStatus < 0) {
  throw new Error(`git_libgit2_init failed with status ${initStatus}`)
}

await printResult(
  {
    ok: true,
    target: options.target,
    libraryPath,
    manifestPath,
    sha256: await sha256(libraryPath),
  },
  options.json,
)
