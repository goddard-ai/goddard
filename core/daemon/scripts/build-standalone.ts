#!/usr/bin/env bun

import { spawnSync } from "node:child_process"
import { createHash } from "node:crypto"
import { mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises"
import { dirname, join, relative, resolve } from "node:path"

import pkg from "../package.json" with { type: "json" }

type StandaloneArtifact = {
  sourcePath: string
  outputPath: string
}

const packageDir = resolve(import.meta.dirname, "..")
const distDir = join(packageDir, "dist")

/** Builds standalone Bun executables for the daemon runtime and helper tools. */
async function main() {
  const args = parseArgs(process.argv.slice(2))
  const target = args.get("target") ?? resolveDefaultCompileTarget()
  const outputDir = resolve(
    process.cwd(),
    args.get("out-dir") ?? join(packageDir, "dist", "standalone", target),
  )
  const executableExt = target.includes("windows") ? ".exe" : ""
  const artifacts: StandaloneArtifact[] = [
    {
      sourcePath: join(distDir, "main.mjs"),
      outputPath: join(outputDir, "bin", `goddard-daemon${executableExt}`),
    },
    {
      sourcePath: join(distDir, "bin", "goddard-tool.mjs"),
      outputPath: join(outputDir, "agent-bin", `goddard${executableExt}`),
    },
    {
      sourcePath: join(distDir, "bin", "workforce-tool.mjs"),
      outputPath: join(outputDir, "agent-bin", `workforce${executableExt}`),
    },
  ]

  runBun(["run", "build"], packageDir)
  await rm(outputDir, { recursive: true, force: true })

  for (const artifact of artifacts) {
    await mkdir(dirname(artifact.outputPath), { recursive: true })
    runBun(buildCompileArgs(target, artifact.sourcePath, artifact.outputPath), packageDir)
  }

  const runtimeHash = createHash("sha256")

  for (const artifact of artifacts) {
    runtimeHash.update(await readFile(artifact.outputPath))
  }

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
      },
      null,
      2,
    ) + "\n",
    "utf8",
  )
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
function runBun(args: string[], cwd: string) {
  const result = spawnSync(process.execPath, args, {
    cwd,
    stdio: "inherit",
    env: process.env,
  })

  if (result.status !== 0) {
    throw new Error(`bun ${args.join(" ")} failed with exit code ${result.status ?? 1}`)
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

await main()
