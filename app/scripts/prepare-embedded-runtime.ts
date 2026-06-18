#!/usr/bin/env bun
import { chmod, cp, mkdir, readFile, realpath, rm, writeFile } from "node:fs/promises"
import { basename, dirname, join, resolve } from "node:path"

import {
  embeddedRuntimeDirName,
  embeddedServicemanVersion,
  type EmbeddedRuntimeManifest,
} from "../src/bun/embedded-runtime-manifest.ts"

type ElectrobunTargetOs = "macos" | "linux" | "win"
type ElectrobunTargetArch = "arm64" | "x64"

type DaemonManifest = {
  formatVersion: 1
  target: string
  version: string
  runtimeHash: string
  executablePath: string
  agentBinDir: string
  helperPaths: {
    goddard: string
    workforce: string
  }
  nativeLibraries?: EmbeddedRuntimeManifest["daemon"]["nativeLibraries"]
}

const appDir = resolve(import.meta.dirname, "..")
const workspaceDir = resolve(appDir, "..")
const coreDaemonDir = resolve(workspaceDir, "core", "daemon")
const embeddedRuntimeDir = join(appDir, ".generated", embeddedRuntimeDirName)
const nativeRuntimeStagingDir = join(appDir, ".generated", "native-runtime")
const macosArm64HomebrewPrefix = "/opt/homebrew"

/** Builds and stages the daemon runtime payload copied into Electrobun resources. */
async function main() {
  if (process.env.NODE_ENV === "development") {
    await rm(embeddedRuntimeDir, { recursive: true, force: true })
    return
  }

  const os = resolveTargetOs()
  const arch = resolveTargetArch()
  const bunTarget = resolveBunCompileTarget(os, arch)
  const daemonOutputDir = join(embeddedRuntimeDir, "daemon")
  const servicemanOutputDir = join(embeddedRuntimeDir, "serviceman")

  await rm(embeddedRuntimeDir, { recursive: true, force: true })
  await rm(nativeRuntimeStagingDir, { recursive: true, force: true })
  await mkdir(embeddedRuntimeDir, { recursive: true })
  const nativeLibraryBuildArgs = await stageNativeLibraryBuildArgs(os, arch)

  runBunScript(coreDaemonDir, [
    join(coreDaemonDir, "scripts", "build-standalone.ts"),
    "--target",
    bunTarget,
    "--out-dir",
    daemonOutputDir,
    ...(os === "macos" ? ["--runtime", "shared-bun"] : []),
    ...nativeLibraryBuildArgs,
  ])

  const daemonManifest = JSON.parse(
    await readFile(join(daemonOutputDir, "manifest.json"), "utf8"),
  ) as DaemonManifest

  await stageServiceman(servicemanOutputDir)

  const manifest: EmbeddedRuntimeManifest = {
    formatVersion: 1,
    target: {
      os,
      arch,
      bunTarget,
    },
    daemon: {
      version: daemonManifest.version,
      runtimeHash: daemonManifest.runtimeHash,
      executablePath: join("daemon", daemonManifest.executablePath),
      agentBinDir: join("daemon", daemonManifest.agentBinDir),
      helperPaths: {
        goddard: join("daemon", daemonManifest.helperPaths.goddard),
        workforce: join("daemon", daemonManifest.helperPaths.workforce),
      },
      ...(daemonManifest.nativeLibraries && {
        nativeLibraries: prefixNativeLibraryPaths("daemon", daemonManifest.nativeLibraries),
      }),
    },
    serviceman: {
      version: embeddedServicemanVersion,
      launcherPath: join("serviceman", "bin", "serviceman"),
      shareDir: join("serviceman", "share", "serviceman"),
    },
  }

  await writeFile(
    join(embeddedRuntimeDir, "manifest.json"),
    JSON.stringify(manifest, null, 2) + "\n",
    "utf8",
  )
}

async function stageNativeLibraryBuildArgs(os: ElectrobunTargetOs, arch: ElectrobunTargetArch) {
  if (os !== "macos" || arch !== "arm64") {
    return []
  }

  const bundle = await stageMacosArm64HomebrewLibgit2Bundle(
    join(nativeRuntimeStagingDir, "libgit2-darwin-arm64"),
  )

  return [
    "--native-libgit2-source-dir",
    bundle.sourceDir,
    "--native-libgit2-library",
    bundle.library,
    "--native-libgit2-version",
    bundle.version,
  ]
}

/** Stages the first packaged libgit2 slice from Homebrew's macOS arm64 bottle layout. */
async function stageMacosArm64HomebrewLibgit2Bundle(outputDir: string) {
  const libgit2Path = join(macosArm64HomebrewPrefix, "lib", "libgit2.dylib")
  const copied = new Map<string, string>()
  const pending = [libgit2Path]

  await mkdir(outputDir, { recursive: true })

  while (pending.length > 0) {
    const sourcePath = pending.pop()!
    const outputPath = join(outputDir, basename(sourcePath))
    if (copied.has(sourcePath)) {
      continue
    }

    await cp(await realpath(sourcePath), outputPath, { force: true })
    copied.set(sourcePath, outputPath)

    for (const dependency of readHomebrewDylibDependencies(sourcePath)) {
      if (!copied.has(dependency)) {
        pending.push(dependency)
      }
    }
  }

  for (const [sourcePath, outputPath] of copied) {
    rewriteBundledDylibPaths(outputPath, readHomebrewDylibDependencies(sourcePath), copied)
    codesignBundledDylib(outputPath)
  }

  return {
    sourceDir: outputDir,
    library: basename(libgit2Path),
    version: resolveHomebrewFormulaVersion("libgit2"),
  }
}

function readHomebrewDylibDependencies(path: string) {
  return runTextCommand(["otool", "-L", path])
    .split("\n")
    .slice(2)
    .map((line) => line.trim().split(/\s+/)[0])
    .filter(
      (dependency) =>
        dependency &&
        dependency !== path &&
        dependency.startsWith(`${macosArm64HomebrewPrefix}/`) &&
        dependency.endsWith(".dylib"),
    )
}

function rewriteBundledDylibPaths(
  outputPath: string,
  dependencies: string[],
  copied: Map<string, string>,
) {
  const args = ["-id", `@loader_path/${basename(outputPath)}`]

  for (const dependency of dependencies) {
    const copiedDependency = copied.get(dependency)
    if (copiedDependency) {
      args.push("-change", dependency, `@loader_path/${basename(copiedDependency)}`)
    }
  }

  runManagedCommand(["install_name_tool", ...args, outputPath])
}

function codesignBundledDylib(outputPath: string) {
  runManagedCommand(["codesign", "--force", "--sign", "-", outputPath])
}

function resolveHomebrewFormulaVersion(formula: string) {
  const source = runTextCommand(["brew", "info", formula, "--json=v2"])
  const parsed = JSON.parse(source) as {
    formulae?: Array<{
      versions?: {
        stable?: string
      }
    }>
  }
  const version = parsed.formulae?.[0]?.versions?.stable
  if (!version) {
    throw new Error(`Homebrew did not report a stable ${formula} version`)
  }
  return version
}

function prefixNativeLibraryPaths(
  prefix: string,
  nativeLibraries: NonNullable<EmbeddedRuntimeManifest["daemon"]["nativeLibraries"]>,
) {
  return {
    ...(nativeLibraries.libgit2 && {
      libgit2: {
        ...nativeLibraries.libgit2,
        path: join(prefix, nativeLibraries.libgit2.path),
      },
    }),
  }
}

/** Downloads the pinned serviceman launcher and templates into the app bundle staging directory. */
async function stageServiceman(outputDir: string) {
  const launcherPath = join(outputDir, "bin", "serviceman")
  const shareDir = join(outputDir, "share", "serviceman")
  const rawBaseUrl = `https://raw.githubusercontent.com/bnnanet/serviceman/${embeddedServicemanVersion}`
  const templateFileNames = [
    "template.agent.plist",
    "template.daemon.plist",
    "template.logrotate",
    "template.openrc",
    "template.system.service",
    "template.user.service",
  ]

  await mkdir(dirname(launcherPath), { recursive: true })
  await mkdir(shareDir, { recursive: true })

  await downloadToFile(`${rawBaseUrl}/bin/serviceman`, launcherPath)
  await chmod(launcherPath, 0o755)

  await Promise.all(
    templateFileNames.map((fileName) =>
      downloadToFile(`${rawBaseUrl}/share/serviceman/${fileName}`, join(shareDir, fileName)),
    ),
  )
}

/** Downloads one static upstream asset and writes it into the local staging directory. */
async function downloadToFile(url: string, destinationPath: string) {
  const response = await fetch(url)

  if (!response.ok) {
    throw new Error(`Failed to download ${url}: ${response.status} ${response.statusText}`)
  }

  await writeFile(destinationPath, await response.text(), "utf8")
}

/** Returns the Bun standalone target string used for the current Electrobun build target. */
function resolveBunCompileTarget(os: ElectrobunTargetOs, arch: ElectrobunTargetArch) {
  const bunOs = os === "macos" ? "darwin" : os === "win" ? "windows" : "linux"
  return `bun-${bunOs}-${arch}`
}

/** Resolves the target OS from Electrobun build env vars or the local host for direct script runs. */
function resolveTargetOs() {
  if (process.env.ELECTROBUN_OS) {
    return process.env.ELECTROBUN_OS as ElectrobunTargetOs
  }

  return process.platform === "darwin" ? "macos" : process.platform === "win32" ? "win" : "linux"
}

/** Resolves the target architecture from Electrobun build env vars or the local host for direct script runs. */
function resolveTargetArch() {
  return (process.env.ELECTROBUN_ARCH ?? process.arch) as ElectrobunTargetArch
}

/** Runs one Bun script with inherited stdio and fails the stage on non-zero exit. */
function runBunScript(cwd: string, args: string[]) {
  const result = Bun.spawnSync([process.execPath, ...args], {
    cwd,
    stdio: ["ignore", "inherit", "inherit"],
    env: process.env,
  })

  if (result.exitCode !== 0) {
    throw new Error(`bun ${args.join(" ")} failed with exit code ${result.exitCode ?? 1}`)
  }
}

function runTextCommand(args: string[]) {
  const result = runManagedCommand(args)
  return result.stdout ? new TextDecoder().decode(result.stdout) : ""
}

function runManagedCommand(args: string[]) {
  const result = Bun.spawnSync(args, {
    stdio: ["ignore", "pipe", "pipe"],
    env: process.env,
  })

  if (result.exitCode !== 0) {
    const stderr = result.stderr ? new TextDecoder().decode(result.stderr) : ""
    const stdout = result.stdout ? new TextDecoder().decode(result.stdout) : ""
    const output = [stdout.trim(), stderr.trim()].filter(Boolean).join("\n")
    throw new Error(output ? `${args.join(" ")} failed:\n${output}` : `${args.join(" ")} failed`)
  }

  return result
}

await main()
