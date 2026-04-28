/** Desktop host helpers that stage, install, and launch the app-managed daemon runtime. */
import { cp, mkdir, mkdtemp, readFile, rename, rm, stat, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"
import { createDaemonIpcClient, resolveDaemonUrl } from "@goddard-ai/daemon-client/node"
import { getGoddardGlobalDir } from "@goddard-ai/paths/node"
import { readDaemonTcpAddressFromDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { Updater } from "electrobun/bun"

import {
  daemonServiceName,
  embeddedRuntimeDirName,
  type EmbeddedRuntimeManifest,
} from "./embedded-runtime-manifest.ts"

type InstalledDaemonState = {
  runtimeHash: string
}

type PreparedDaemonRuntime = {
  daemonRootDir: string
  agentBinDir: string
  daemonExecutablePath: string
  runtimeHash: string
}

const daemonInstallRootDir = join(getGoddardGlobalDir(), "desktop-runtime")
const daemonInstallStatePath = join(daemonInstallRootDir, "installed-daemon.json")
const daemonInstallVersionsDir = join(daemonInstallRootDir, "daemon-installs")

let ensuredRuntime: Promise<{ daemonUrl: string }> | undefined

/** Ensures the desktop-managed daemon install is current, registered, and accepting IPC traffic. */
export function ensureDaemonRuntime() {
  ensuredRuntime ??= ensureDaemonRuntimeInner().catch((error) => {
    ensuredRuntime = undefined
    throw error
  })
  return ensuredRuntime
}

/** Reads the app-bundled manifest, installs runtime files, and starts or updates the daemon service. */
async function ensureDaemonRuntimeInner() {
  const daemon = resolveDaemonConnection()

  if (isDevelopmentRuntime()) {
    await ensureDevelopmentDaemonRuntime(daemon.daemonUrl)
    return { daemonUrl: daemon.daemonUrl }
  }

  const manifest = await readEmbeddedRuntimeManifest()
  const baseUrl = await resolveDaemonBaseUrl()
  const installedState = await readInstalledDaemonState()
  const preparedRuntime = await prepareDaemonRuntime(manifest)

  if (
    installedState?.runtimeHash === preparedRuntime.runtimeHash &&
    (await pingDaemon(daemon.daemonUrl).catch(() => false))
  ) {
    return { daemonUrl: daemon.daemonUrl }
  }

  if (process.platform === "win32") {
    await installWindowsDaemonStartup(preparedRuntime, baseUrl, daemon.port)
  } else {
    await installUnixDaemonService(manifest, preparedRuntime, baseUrl, daemon.port)
  }

  await waitForDaemonReady(daemon.daemonUrl)
  await writeInstalledDaemonState({ runtimeHash: preparedRuntime.runtimeHash })

  return { daemonUrl: daemon.daemonUrl }
}

/** In development, reuse the separately watched daemon process instead of the app-bundled runtime. */
async function ensureDevelopmentDaemonRuntime(daemonUrl: string) {
  try {
    await waitForDaemonReady(daemonUrl, 5_000)
  } catch {
    throw new Error(
      "Development mode expects a running Goddard daemon. Start `bun run dev` from the workspace root, or launch `core/daemon` before starting the app.",
    )
  }
}

/** Reads the app-bundled daemon runtime manifest copied into Electrobun resources. */
async function readEmbeddedRuntimeManifest() {
  return JSON.parse(
    await readFile(join(resolveEmbeddedRuntimeRoot(), "manifest.json"), "utf8"),
  ) as EmbeddedRuntimeManifest
}

/** Returns whether the Bun host should reuse the external development daemon. */
function isDevelopmentRuntime() {
  return (
    process.env.NODE_ENV === "development" ||
    Bun.env.NODE_ENV === "development" ||
    Bun.argv.some((argument) => argument === "--watch" || argument === "dev")
  )
}

/** Returns the Electrobun resource directory containing the daemon runtime payload. */
function resolveEmbeddedRuntimeRoot() {
  return resolve("..", "Resources", "app", embeddedRuntimeDirName)
}

/** Returns the backend base URL used by the desktop-managed daemon service. */
async function resolveDaemonBaseUrl() {
  if (process.env.GODDARD_BASE_URL) {
    return process.env.GODDARD_BASE_URL
  }

  const channel = await Updater.localInfo.channel()
  return channel === "dev" ? "http://127.0.0.1:8787" : "https://goddardai.org/api"
}

/** Returns the daemon data profile the desktop host should install for the active app channel. */
async function resolveDaemonDataProfile() {
  if (process.env.GODDARD_DATA_PROFILE) {
    return process.env.GODDARD_DATA_PROFILE
  }

  const channel = await Updater.localInfo.channel()
  return channel === "dev" ? "development" : undefined
}

/** Reads the last runtime hash installed by the desktop app when present. */
async function readInstalledDaemonState() {
  const source = await readFile(daemonInstallStatePath, "utf8").catch(() => null)
  return source ? (JSON.parse(source) as InstalledDaemonState) : null
}

/** Writes the runtime hash installed by the current app bundle for future startup checks. */
async function writeInstalledDaemonState(state: InstalledDaemonState) {
  await mkdir(dirname(daemonInstallStatePath), { recursive: true })
  await writeFile(daemonInstallStatePath, JSON.stringify(state, null, 2) + "\n", "utf8")
}

/** Copies the bundled daemon runtime into one versioned install directory when needed. */
async function prepareDaemonRuntime(manifest: EmbeddedRuntimeManifest) {
  const installDir = join(daemonInstallVersionsDir, manifest.daemon.runtimeHash)
  const embeddedDaemonRootDir = join(resolveEmbeddedRuntimeRoot(), "daemon")

  if (!(await pathExists(installDir))) {
    await mkdir(daemonInstallVersionsDir, { recursive: true })
    const stagingRoot = await mkdtemp(join(tmpdir(), "goddard-daemon-install-"))
    const stagedInstallDir = join(stagingRoot, "runtime")

    try {
      await cp(embeddedDaemonRootDir, stagedInstallDir, { recursive: true, force: true })
      await rename(stagedInstallDir, installDir)
    } catch (error) {
      await rm(stagingRoot, { recursive: true, force: true }).catch(() => {})
      throw error
    }
  }

  return {
    daemonRootDir: installDir,
    daemonExecutablePath: join(installDir, manifest.daemon.executablePath),
    agentBinDir: join(installDir, manifest.daemon.agentBinDir),
    runtimeHash: manifest.daemon.runtimeHash,
  } satisfies PreparedDaemonRuntime
}

/** Installs or updates a user-scoped daemon service through the bundled serviceman shell launcher. */
async function installUnixDaemonService(
  manifest: EmbeddedRuntimeManifest,
  runtime: PreparedDaemonRuntime,
  baseUrl: string,
  daemonPort: number,
) {
  const dataProfile = await resolveDaemonDataProfile()
  const servicemanLauncherPath = join(
    resolveEmbeddedRuntimeRoot(),
    manifest.serviceman.launcherPath,
  )
  const args = [
    "/bin/sh",
    servicemanLauncherPath,
    "add",
    "--agent",
    "--force",
    "--name",
    daemonServiceName,
    "--title",
    "Goddard Daemon",
    "--desc",
    "Goddard desktop background daemon",
    "--workdir",
    runtime.daemonRootDir,
    "--path",
    process.env.PATH ?? "",
  ]

  if (process.platform === "darwin") {
    args.push("--rdns", "app.goddardai.org")
  }

  args.push(
    "--",
    runtime.daemonExecutablePath,
    "run",
    "--base-url",
    baseUrl,
    "--port",
    String(daemonPort),
    "--agent-bin-dir",
    runtime.agentBinDir,
  )

  if (dataProfile) {
    args.push("--data-profile", dataProfile)
  }

  runManagedCommand(args, {
    PATH: process.env.PATH ?? "",
    HOME: process.env.HOME,
  })
}

/** Installs the daemon autostart entry in the user Run registry and launches the current binary now. */
async function installWindowsDaemonStartup(
  runtime: PreparedDaemonRuntime,
  baseUrl: string,
  daemonPort: number,
) {
  const dataProfile = await resolveDaemonDataProfile()
  const daemonArgs = [
    runtime.daemonExecutablePath,
    "run",
    "--base-url",
    baseUrl,
    "--port",
    String(daemonPort),
    "--agent-bin-dir",
    runtime.agentBinDir,
  ]

  if (dataProfile) {
    daemonArgs.push("--data-profile", dataProfile)
  }

  const runKeyCommand = daemonArgs.map(quoteWindowsCommandArgument).join(" ")

  runManagedCommand([
    "reg",
    "add",
    "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run",
    "/v",
    daemonServiceName,
    "/t",
    "REG_SZ",
    "/d",
    runKeyCommand,
    "/f",
  ])

  runManagedCommand(["taskkill", "/F", "/IM", "goddard-daemon.exe"], {
    ignoreFailure: true,
  })

  const subprocess = Bun.spawn(daemonArgs, {
    detached: true,
    stdio: ["ignore", "ignore", "ignore"],
    env: process.env,
  })
  subprocess.unref()
}

/** Waits for the daemon IPC endpoint to accept health checks after install or restart. */
async function waitForDaemonReady(daemonUrl: string, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs

  while (Date.now() < deadline) {
    if (await pingDaemon(daemonUrl).catch(() => false)) {
      return
    }

    await Bun.sleep(250)
  }

  throw new Error(`Timed out waiting for the Goddard daemon at ${daemonUrl}`)
}

/** Sends a daemon health check request and returns whether the daemon answered successfully. */
async function pingDaemon(daemonUrl: string) {
  const client = createDaemonIpcClient({ daemonUrl })
  const response = await client.send("daemon.health")
  return response.ok === true
}

/** Spawns one managed setup command and throws with captured output when it fails. */
function runManagedCommand(
  args: string[],
  options: {
    PATH?: string
    HOME?: string
    ignoreFailure?: boolean
  } = {},
) {
  const result = Bun.spawnSync(args, {
    stdio: ["ignore", "pipe", "pipe"],
    env: {
      ...process.env,
      PATH: options.PATH ?? process.env.PATH,
      HOME: options.HOME ?? process.env.HOME,
    },
  })

  if (result.exitCode === 0 || options.ignoreFailure) {
    return
  }

  const stderr = result.stderr ? new TextDecoder().decode(result.stderr) : ""
  const stdout = result.stdout ? new TextDecoder().decode(result.stdout) : ""
  const output = [stdout.trim(), stderr.trim()].filter(Boolean).join("\n")
  throw new Error(output ? `${args.join(" ")} failed:\n${output}` : `${args.join(" ")} failed`)
}

/** Applies Windows command-line quoting so registry startup values survive spaces and quotes. */
function quoteWindowsCommandArgument(value: string) {
  if (!/[ \t"]/.test(value)) {
    return value
  }

  let escaped = '"'
  let backslashes = 0

  for (const character of value) {
    if (character === "\\") {
      backslashes += 1
      continue
    }

    if (character === '"') {
      escaped += "\\".repeat(backslashes * 2 + 1)
      escaped += '"'
      backslashes = 0
      continue
    }

    escaped += "\\".repeat(backslashes)
    escaped += character
    backslashes = 0
  }

  escaped += "\\".repeat(backslashes * 2)
  escaped += '"'
  return escaped
}

/** Checks whether one filesystem path currently exists. */
async function pathExists(path: string) {
  return Boolean(await stat(path).catch(() => null))
}

function resolveDaemonConnection() {
  const daemonUrl = resolveDaemonUrl()
  const address = readDaemonTcpAddressFromDaemonUrl(daemonUrl)

  return {
    daemonUrl,
    port: address.port,
  }
}
