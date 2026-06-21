import { join } from "node:path"

import {
  artifactManifestPath,
  artifactPath,
  assertFile,
  buildDir,
  copyLicenseFiles,
  distDir,
  ensureDir,
  parseOptions,
  pathExists,
  readVersions,
  resolveTarget,
  rootDir,
  run,
  sha256,
  sourceDir,
  writeJson,
} from "./common.ts"

const options = parseOptions()
const targetConfig = await resolveTarget(options.target)
const versions = await readVersions()

if (!(await pathExists(sourceDir))) {
  throw new Error(
    `Missing libgit2 source at ${sourceDir}. Run pnpm --dir native/libgit2 run fetch first.`,
  )
}

const targetBuildDir = buildDir(options.target)
const targetDistDir = distDir(options.target)
await ensureDir(targetBuildDir)
await ensureDir(targetDistDir)

const cmakeArgs = [
  "-S",
  sourceDir,
  "-B",
  targetBuildDir,
  "-DCMAKE_BUILD_TYPE=Release",
  `-DCMAKE_INSTALL_PREFIX=${targetDistDir}`,
  "-DBUILD_SHARED_LIBS=ON",
  "-DBUILD_TESTS=OFF",
  "-DBUILD_CLI=OFF",
  "-DBUILD_EXAMPLES=OFF",
  "-DUSE_SSH=OFF",
  "-DUSE_HTTPS=OFF",
  "-DUSE_NTLMCLIENT=OFF",
  "-DUSE_BUNDLED_ZLIB=ON",
  "-DREGEX_BACKEND=builtin",
]

if (targetConfig.platform === "darwin") {
  cmakeArgs.push(`-DCMAKE_OSX_ARCHITECTURES=${targetConfig.arch}`)
}

await run("cmake", cmakeArgs)
await run("cmake", ["--build", targetBuildDir, "--config", "Release", "--parallel"])
await run("cmake", ["--install", targetBuildDir, "--config", "Release"])

const libraryPath = await artifactPath(options.target)
await assertFile(libraryPath)
await copyLicenseFiles(options.target)

const manifestPath = await artifactManifestPath(options.target)
const sourceCommit = (await run("git", ["rev-parse", "HEAD"], { cwd: sourceDir })).stdout.trim()
await writeJson(manifestPath, {
  target: options.target,
  platform: targetConfig.platform,
  arch: targetConfig.arch,
  bunTarget: targetConfig.bunTarget,
  libgit2Version: versions.libgit2.version,
  artifact: targetConfig.library,
  sha256: await sha256(libraryPath),
  source: {
    repo: versions.libgit2.repo,
    ref: versions.libgit2.ref,
    commit: sourceCommit,
  },
})

console.log(`Built ${join(rootDir, "dist", options.target)}`)
