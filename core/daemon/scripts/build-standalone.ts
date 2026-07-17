#!/usr/bin/env bun
import { createHash } from "node:crypto"
import { chmod, cp, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises"
import { createRequire } from "node:module"
import { dirname, isAbsolute, join, relative, resolve } from "node:path"

import {
  nativeLibgit2Manifest,
  nativeLibgit2TargetForBunTarget,
  type NativeLibgit2Target,
} from "../../libgit2/vendor/libgit2/manifest.ts"
import pkg from "../package.json" with { type: "json" }

type StandaloneArtifact = {
  sourcePath: string
  outputPath: string
  runtime?: "compiled" | "shared-bun"
}

export type NativeLibrariesManifest = {
  libgit2?: {
    target: string
    path: string
    version?: string
    sha256: string
  }
}

type NativeLibgit2Artifact = {
  target: NativeLibgit2Target
  sourceDir: string
  library: string
  version: string
}

type GeneratedNativeLibgit2Manifest = {
  target: string
  bunTarget: string
  libgit2Version: string
  artifact: string
}

const packageDir = resolve(import.meta.dirname, "..")
const workspaceDir = resolve(packageDir, "..", "..")
const distDir = join(packageDir, "dist")
const libgit2VendorDir = resolve(packageDir, "..", "libgit2", "vendor", "libgit2")
const nodeStreamZipDir = dirname(
  createRequire(import.meta.url).resolve("node-stream-zip/package.json"),
)
const sessionPromptsDir = resolve(
  packageDir,
  "..",
  "..",
  "features",
  "session",
  "src",
  "daemon",
  "prompts",
)

/** Builds standalone Bun executables for the daemon runtime and helper tools. */
export async function main() {
  const args = parseArgs(process.argv.slice(2))
  const target = args.get("target") ?? resolveDefaultCompileTarget()
  const runtime = resolveArtifactRuntime(args.get("runtime"), "--runtime")
  const helperRuntime = resolveArtifactRuntime(
    args.get("helper-runtime") ?? runtime,
    "--helper-runtime",
  )
  const outputDir = resolve(
    process.cwd(),
    args.get("out-dir") ?? join(packageDir, "dist", "standalone", target),
  )
  const executableExt = target.includes("windows") ? ".exe" : ""
  const daemonExt = runtime === "shared-bun" ? "" : executableExt
  const helperExt = helperRuntime === "shared-bun" ? "" : executableExt
  const nativeLibgit2Artifact = await prepareNativeLibgit2Artifact(target)
  const artifacts: StandaloneArtifact[] = [
    {
      sourcePath: join(distDir, "main.mjs"),
      outputPath: join(outputDir, "bin", `goddard-daemon${daemonExt}`),
      runtime,
    },
    {
      sourcePath: join(distDir, "bin", "goddard-tool.mjs"),
      outputPath: join(outputDir, "agent-bin", `goddard${helperExt}`),
      runtime: helperRuntime === "shared-bun" ? "shared-bun" : "compiled",
    },
    {
      sourcePath: join(distDir, "bin", "workforce-tool.mjs"),
      outputPath: join(outputDir, "agent-bin", `workforce${helperExt}`),
      runtime: helperRuntime === "shared-bun" ? "shared-bun" : "compiled",
    },
  ]

  await runWorkspaceDaemonBuild()
  await rm(outputDir, { recursive: true, force: true })
  const sharedBunArtifacts = artifacts.filter((artifact) => artifact.runtime === "shared-bun")
  const sharedBunFiles =
    sharedBunArtifacts.length > 0
      ? await stageSharedBunRuntime({
          outputDir,
          entrypoints: sharedBunArtifacts.map((artifact) => ({
            sourcePath: artifact.sourcePath,
            outputPath: relative(distDir, artifact.sourcePath),
          })),
          packages: [{ name: "node-stream-zip", sourceDir: nodeStreamZipDir }],
          directories: [{ sourceDir: sessionPromptsDir, outputPath: "prompts" }],
        })
      : []

  for (const artifact of artifacts) {
    await mkdir(dirname(artifact.outputPath), { recursive: true })

    if (artifact.runtime === "shared-bun") {
      await writeSharedBunHelper(artifact, outputDir)
    } else {
      await runBun(buildCompileArgs(target, artifact.sourcePath, artifact.outputPath), packageDir)
    }
  }

  const nativeLibraries = await stageNativeLibraries({
    outputDir,
    libgit2: nativeLibgit2Artifact,
  })
  const runtimeHash = createHash("sha256")

  for (const artifact of artifacts) {
    runtimeHash.update(await readFile(artifact.outputPath))
  }
  for (const sharedBunFile of sharedBunFiles) {
    runtimeHash.update(relativeFromOutputDir(outputDir, sharedBunFile))
    runtimeHash.update(await readFile(sharedBunFile))
  }
  await updateNativeLibrariesHash(runtimeHash, outputDir, nativeLibraries)

  await cleanupBunBuildScratchFiles()

  await writeFile(
    join(outputDir, "manifest.json"),
    JSON.stringify(
      {
        formatVersion: 1,
        target,
        version: pkg.version,
        runtimeHash: runtimeHash.digest("hex"),
        executablePath: relativeFromOutputDir(outputDir, artifacts[0]!.outputPath),
        agentBinDir: "agent-bin",
        helperPaths: {
          goddard: relativeFromOutputDir(outputDir, artifacts[1]!.outputPath),
          workforce: relativeFromOutputDir(outputDir, artifacts[2]!.outputPath),
        },
        ...(sharedBunArtifacts.length > 0 && {
          sharedBunLauncherPaths: sharedBunArtifacts.map((artifact) =>
            relativeFromOutputDir(outputDir, artifact.outputPath),
          ),
        }),
        ...(Object.keys(nativeLibraries).length > 0 && { nativeLibraries }),
      },
      null,
      2,
    ) + "\n",
    "utf8",
  )
}

/** Stages optional native runtime libraries into the standalone output directory. */
export async function stageNativeLibraries(input: {
  outputDir: string
  libgit2?: NativeLibgit2Artifact
}) {
  const nativeLibraries: NativeLibrariesManifest = {}

  if (!input.libgit2) {
    return nativeLibraries
  }

  const sourceDir = resolve(process.cwd(), input.libgit2.sourceDir)
  const librarySourcePath = resolve(sourceDir, input.libgit2.library)
  if (!isInsideOrEqual(sourceDir, librarySourcePath)) {
    throw new Error("The native libgit2 library must be inside its artifact directory")
  }

  const outputNativeDir = join(input.outputDir, "native", "libgit2")
  await mkdir(dirname(outputNativeDir), { recursive: true })
  await cp(sourceDir, outputNativeDir, { recursive: true, force: true })

  const libraryOutputPath = join(outputNativeDir, relative(sourceDir, librarySourcePath))
  const sha256 = createHash("sha256")
    .update(await readFile(libraryOutputPath))
    .digest("hex")

  nativeLibraries.libgit2 = {
    target: input.libgit2.target,
    path: relativeFromOutputDir(input.outputDir, libraryOutputPath),
    version: input.libgit2.version,
    sha256,
  }

  return nativeLibraries
}

/** Resolves the required package-owned native artifact for a Bun compile target. */
export function resolveNativeLibgit2Target(target: string) {
  const nativeTarget = nativeLibgit2TargetForBunTarget(target)
  if (nativeTarget) {
    return nativeTarget
  }

  const supportedTargets = Object.values(nativeLibgit2Manifest.targets)
    .map((candidate) => candidate.bunTarget)
    .join(", ")
  throw new Error(
    `No packaged libgit2 artifact supports ${target}. Supported Bun targets: ${supportedTargets}`,
  )
}

/** Builds and verifies the package-owned libgit2 artifact required by the daemon target. */
async function prepareNativeLibgit2Artifact(target: string): Promise<NativeLibgit2Artifact> {
  const nativeTarget = resolveNativeLibgit2Target(target)
  const targetConfig = nativeLibgit2Manifest.targets[nativeTarget]
  await runBun(
    ["run", join(libgit2VendorDir, "scripts", "prepare-runtime.ts"), "--target", nativeTarget],
    libgit2VendorDir,
  )

  const sourceDir = join(libgit2VendorDir, "dist", nativeTarget)
  const manifest = JSON.parse(
    await readFile(join(sourceDir, "manifest.json"), "utf8"),
  ) as GeneratedNativeLibgit2Manifest
  if (
    manifest.target !== nativeTarget ||
    manifest.bunTarget !== target ||
    manifest.artifact !== targetConfig.library
  ) {
    throw new Error(`Generated libgit2 manifest does not match daemon target ${target}`)
  }

  return {
    target: nativeTarget,
    sourceDir,
    library: manifest.artifact,
    version: manifest.libgit2Version,
  }
}

/** Lists every file that belongs to a staged native library payload. */
export async function listNativeLibraryFiles(
  outputDir: string,
  nativeLibraries: NativeLibrariesManifest,
) {
  const roots = new Set<string>()

  if (nativeLibraries.libgit2) {
    roots.add(join(outputDir, "native", "libgit2"))
  }

  const files: string[] = []
  for (const root of roots) {
    files.push(...(await listFilesRecursive(root)))
  }

  return files.sort((left, right) =>
    relativeFromOutputDir(outputDir, left).localeCompare(relativeFromOutputDir(outputDir, right)),
  )
}

/** Adds staged native library paths and bytes to the standalone runtime hash. */
export async function updateNativeLibrariesHash(
  runtimeHash: ReturnType<typeof createHash>,
  outputDir: string,
  nativeLibraries: NativeLibrariesManifest,
) {
  for (const nativeFilePath of await listNativeLibraryFiles(outputDir, nativeLibraries)) {
    runtimeHash.update(relativeFromOutputDir(outputDir, nativeFilePath))
    runtimeHash.update(await readFile(nativeFilePath))
  }
}

/** Resolves how command artifacts are emitted for the standalone runtime. */
function resolveArtifactRuntime(value: string | undefined, flagName: string) {
  if (!value || value === "compiled") {
    return "compiled"
  }

  if (value === "shared-bun") {
    return "shared-bun"
  }

  throw new Error(`${flagName} must be either compiled or shared-bun`)
}

/** Writes a small launcher plus bundled JS payload for app builds that already ship Bun. */
async function writeSharedBunHelper(artifact: StandaloneArtifact, outputDir: string) {
  const payloadPath = join(outputDir, "runtime", relative(distDir, artifact.sourcePath))
  const relativePayloadPath = relative(dirname(artifact.outputPath), payloadPath).replaceAll(
    "\\",
    "/",
  )
  await writeFile(
    artifact.outputPath,
    [
      "#!/bin/sh",
      'launcher_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"',
      `exec "\${GODDARD_BUN_RUNTIME:-bun}" "$launcher_dir/${relativePayloadPath}" "$@"`,
      "",
    ].join("\n"),
    "utf8",
  )
  await chmod(artifact.outputPath, 0o755)
}

/** Stages the complete bundled module graph used by shared-Bun launchers. */
export async function stageSharedBunRuntime(input: {
  outputDir: string
  entrypoints: Array<{ sourcePath: string; outputPath: string }>
  packages?: Array<{ name: string; sourceDir: string }>
  directories?: Array<{ sourceDir: string; outputPath: string }>
}) {
  const outputDir = resolve(input.outputDir)
  const runtimeDir = join(outputDir, "runtime")
  await rm(runtimeDir, { recursive: true, force: true })

  for (const entrypoint of input.entrypoints) {
    const sourcePath = resolve(entrypoint.sourcePath)
    const outputPath = resolve(runtimeDir, entrypoint.outputPath)
    if (!isInsideOrEqual(runtimeDir, outputPath)) {
      throw new Error("A shared Bun entrypoint output must be inside the runtime directory")
    }
    await mkdir(dirname(outputPath), { recursive: true })
    await runBun(
      ["build", sourcePath, "--target=bun", "--packages=bundle", `--outfile=${outputPath}`],
      packageDir,
    )
  }

  for (const runtimePackage of input.packages ?? []) {
    const packageOutputDir = join(runtimeDir, "node_modules", runtimePackage.name)
    await mkdir(dirname(packageOutputDir), { recursive: true })
    await cp(runtimePackage.sourceDir, packageOutputDir, {
      recursive: true,
      force: true,
    })
  }

  for (const directory of input.directories ?? []) {
    const outputPath = resolve(runtimeDir, directory.outputPath)
    if (!isInsideOrEqual(runtimeDir, outputPath)) {
      throw new Error("A shared Bun runtime directory must be inside the runtime directory")
    }
    await cp(directory.sourceDir, outputPath, { recursive: true, force: true })
  }

  return (await listFilesRecursive(runtimeDir)).sort((left, right) =>
    relativeFromOutputDir(runtimeDir, left).localeCompare(relativeFromOutputDir(runtimeDir, right)),
  )
}

async function listFilesRecursive(path: string): Promise<string[]> {
  const entries = await readdir(path, { withFileTypes: true })
  const files: string[] = []

  for (const entry of entries) {
    const entryPath = join(path, entry.name)
    if (entry.isDirectory()) {
      files.push(...(await listFilesRecursive(entryPath)))
    } else if (entry.isFile()) {
      files.push(entryPath)
    }
  }

  return files
}

function isInsideOrEqual(parent: string, child: string) {
  const rel = relative(parent, child)
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel))
}

/** Parses repeated `--key value` command-line arguments into one lookup map. */
function parseArgs(argv: string[]) {
  const args = new Map<string, string>()

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index]

    if (!argument?.startsWith("--")) {
      continue
    }

    const key = argument.slice(2)
    const value = argv[index + 1]

    if (!value || value.startsWith("--")) {
      throw new Error(`Missing value for --${key}`)
    }

    args.set(key, value)
    index += 1
  }

  return args
}

/** Returns the Bun compile target for the current host platform and architecture. */
function resolveDefaultCompileTarget() {
  const os =
    process.platform === "darwin"
      ? "darwin"
      : process.platform === "win32"
        ? "windows"
        : process.platform

  return `bun-${os}-${process.arch}`
}

/** Builds the Bun CLI argument list for one standalone executable output. */
function buildCompileArgs(target: string, sourcePath: string, outputPath: string) {
  const args = [
    "build",
    "--compile",
    `--target=${target}`,
    "--no-compile-autoload-dotenv",
    "--no-compile-autoload-bunfig",
    "--no-compile-autoload-package-json",
    `--outfile=${outputPath}`,
  ]

  if (target.includes("windows")) {
    args.push("--windows-hide-console")
  }

  args.push(sourcePath)
  return args
}

/** Runs one Bun subprocess and fails the build immediately on non-zero exit. */
async function runBun(args: string[], cwd: string) {
  await runCommand(process.execPath, args, cwd)
}

/** Builds the daemon and all workspace dependencies needed by its bundled entrypoints. */
async function runWorkspaceDaemonBuild() {
  await runCommand(
    "pnpm",
    [
      "exec",
      "turbo",
      "run",
      "build",
      "--filter=@goddard-ai/daemon...",
      "--output-logs=errors-only",
    ],
    workspaceDir,
  )
}

async function runCommand(command: string, args: string[], cwd: string) {
  const subprocess = Bun.spawn([command, ...args], {
    cwd,
    stdin: "ignore",
    stdout: "inherit",
    stderr: "inherit",
    env: process.env,
  })
  const exitCode = await subprocess.exited

  if (exitCode !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed with exit code ${exitCode}`)
  }
}

/** Removes temporary `.bun-build` scratch files Bun leaves beside the compile entrypoints. */
async function cleanupBunBuildScratchFiles() {
  const entries = await readdir(packageDir)

  await Promise.all(
    entries
      .filter((entry) => entry.endsWith(".bun-build"))
      .map((entry) => rm(join(packageDir, entry), { recursive: true, force: true })),
  )
}

/** Converts one absolute artifact path into the manifest's output-relative form. */
function relativeFromOutputDir(outputDir: string, artifactPath: string) {
  return relative(outputDir, artifactPath).replaceAll("\\", "/")
}

if (import.meta.main) {
  await main()
}
