#!/usr/bin/env bun
import { createHash } from "node:crypto"
import { chmod, cp, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises"
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path"

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

const packageDir = resolve(import.meta.dirname, "..")
const distDir = join(packageDir, "dist")

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

  await runBun(["run", "build"], packageDir)
  await rm(outputDir, { recursive: true, force: true })

  for (const artifact of artifacts) {
    await mkdir(dirname(artifact.outputPath), { recursive: true })

    if (artifact.runtime === "shared-bun") {
      await writeSharedBunHelper(artifact)
    } else {
      await runBun(buildCompileArgs(target, artifact.sourcePath, artifact.outputPath), packageDir)
    }
  }

  const nativeLibraries = await stageNativeLibraries({
    outputDir,
    target,
    libgit2SourceDir: args.get("native-libgit2-source-dir"),
    libgit2Library: args.get("native-libgit2-library"),
    libgit2Version: args.get("native-libgit2-version"),
  })
  const runtimeHash = createHash("sha256")

  for (const artifact of artifacts) {
    runtimeHash.update(await readFile(artifact.outputPath))
    if (artifact.runtime === "shared-bun") {
      runtimeHash.update(await readFile(`${artifact.outputPath}.mjs`))
    }
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
  target: string
  libgit2SourceDir?: string
  libgit2Library?: string
  libgit2Version?: string
}) {
  const nativeLibraries: NativeLibrariesManifest = {}

  if (!input.libgit2SourceDir && !input.libgit2Library && !input.libgit2Version) {
    return nativeLibraries
  }

  if (!input.libgit2SourceDir || !input.libgit2Library) {
    throw new Error(
      "--native-libgit2-source-dir and --native-libgit2-library must be provided together",
    )
  }

  const sourceDir = resolve(process.cwd(), input.libgit2SourceDir)
  const librarySourcePath = resolve(sourceDir, input.libgit2Library)
  if (!isInsideOrEqual(sourceDir, librarySourcePath)) {
    throw new Error("--native-libgit2-library must point inside --native-libgit2-source-dir")
  }

  const outputNativeDir = join(input.outputDir, "native", "libgit2")
  await mkdir(dirname(outputNativeDir), { recursive: true })
  await cp(sourceDir, outputNativeDir, { recursive: true, force: true })

  const libraryOutputPath = join(outputNativeDir, relative(sourceDir, librarySourcePath))
  const sha256 = createHash("sha256")
    .update(await readFile(libraryOutputPath))
    .digest("hex")

  nativeLibraries.libgit2 = {
    target: input.target,
    path: relativeFromOutputDir(input.outputDir, libraryOutputPath),
    ...(input.libgit2Version && { version: input.libgit2Version }),
    sha256,
  }

  return nativeLibraries
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
async function writeSharedBunHelper(artifact: StandaloneArtifact) {
  const payloadPath = `${artifact.outputPath}.mjs`
  const payloadName = basename(payloadPath)

  await cp(artifact.sourcePath, payloadPath)
  await writeFile(
    artifact.outputPath,
    [
      "#!/bin/sh",
      'launcher_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"',
      `exec "\${GODDARD_BUN_RUNTIME:-bun}" "$launcher_dir/${payloadName}" "$@"`,
      "",
    ].join("\n"),
    "utf8",
  )
  await chmod(artifact.outputPath, 0o755)
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
  const subprocess = Bun.spawn([process.execPath, ...args], {
    cwd,
    stdin: "ignore",
    stdout: "inherit",
    stderr: "inherit",
    env: process.env,
  })
  const exitCode = await subprocess.exited

  if (exitCode !== 0) {
    throw new Error(`bun ${args.join(" ")} failed with exit code ${exitCode}`)
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
