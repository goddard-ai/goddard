#!/usr/bin/env bun
import { spawnSync } from "node:child_process"
import { createHash } from "node:crypto"
import { chmod, cp, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises"
import { basename, dirname, join, relative, resolve } from "node:path"

import pkg from "../package.json" with { type: "json" }

type StandaloneArtifact = {
  sourcePath: string
  outputPath: string
  runtime?: "compiled" | "shared-bun"
}

const packageDir = resolve(import.meta.dirname, "..")
const distDir = join(packageDir, "dist")

/** Builds standalone Bun executables for the daemon runtime and helper tools. */
async function main() {
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

  runBun(["run", "build"], packageDir)
  await rm(outputDir, { recursive: true, force: true })

  for (const artifact of artifacts) {
    await mkdir(dirname(artifact.outputPath), { recursive: true })

    if (artifact.runtime === "shared-bun") {
      await writeSharedBunHelper(artifact)
    } else {
      runBun(buildCompileArgs(target, artifact.sourcePath, artifact.outputPath), packageDir)
    }
  }

  const runtimeHash = createHash("sha256")

  for (const artifact of artifacts) {
    runtimeHash.update(await readFile(artifact.outputPath))
    if (artifact.runtime === "shared-bun") {
      runtimeHash.update(await readFile(`${artifact.outputPath}.mjs`))
    }
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
