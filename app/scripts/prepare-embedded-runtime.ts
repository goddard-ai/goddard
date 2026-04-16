#!/usr/bin/env bun

import { chmod, mkdir, readFile, rm, writeFile } from "node:fs/promises"
import { dirname, join, resolve } from "node:path"

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
}

const appDir = resolve(import.meta.dirname, "..")
const workspaceDir = resolve(appDir, "..")
const coreDaemonDir = resolve(workspaceDir, "core", "daemon")
const embeddedRuntimeDir = join(appDir, ".generated", embeddedRuntimeDirName)

/** Builds and stages the daemon runtime payload copied into Electrobun resources. */
async function main() {
  const os = resolveTargetOs()
  const arch = resolveTargetArch()
  const bunTarget = resolveBunCompileTarget(os, arch)
  const daemonOutputDir = join(embeddedRuntimeDir, "daemon")
  const servicemanOutputDir = join(embeddedRuntimeDir, "serviceman")

  await rm(embeddedRuntimeDir, { recursive: true, force: true })
  await mkdir(embeddedRuntimeDir, { recursive: true })

  runBunScript(coreDaemonDir, [
    join(coreDaemonDir, "scripts", "build-standalone.ts"),
    "--target",
    bunTarget,
    "--out-dir",
    daemonOutputDir,
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

await main()
