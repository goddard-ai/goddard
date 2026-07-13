/** Desktop host helpers that stage, install, and launch the app-managed daemon runtime. */
import { chmod, cp, mkdir, mkdtemp, readFile, rename, rm, stat, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"
import { createDaemonIpcClient, resolveDaemonUrl } from "@goddard-ai/daemon-client/node"
import { getGoddardGlobalDir } from "@goddard-ai/paths/node"
import { readDaemonTcpAddressFromDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { Updater } from "electrobun/bun"

import {
  bindBunRuntimeLauncher,
  createDaemonRunArgs,
  resolveInstalledNativeRuntimePaths,
  type PreparedDaemonRuntime,
} from "./daemon-runtime-launch.ts"
import {
  daemonServiceName,
  embeddedRuntimeDirName,
  type EmbeddedRuntimeManifest,
} from "./embedded-runtime-manifest.ts"
import { getAppDebug, writeAppError, writeAppLog } from "./logging.ts"

type InstalledDaemonState = {
  runtimeHash: string
}

const daemonInstallRootDir = join(getGoddardGlobalDir(), "desktop-runtime")
const daemonInstallStatePath = join(daemonInstallRootDir, "installed-daemon.json")
const daemonInstallVersionsDir = join(daemonInstallRootDir, "daemon-installs")

let ensuredRuntime: Promise<{ daemonUrl: string }> | undefined

/** Ensures the desktop-managed daemon install is current, registered, and accepting IPC traffic. */
export function ensureDaemonRuntime() {
  if (!ensuredRuntime) {
    const startedAt = Date.now()
    const mode = isDevelopmentRuntime() ? "development" : "packaged"
    ensuredRuntime = ensureDaemonRuntimeInner()
      .then((runtime) => {
        writeAppLog({
          source: "host",
          level: "info",
          message: "app.daemon.ready",
          properties: {
            daemonUrl: runtime.daemonUrl,
            durationMs: Date.now() - startedAt,
            mode,
          },
        })
        return runtime
      })
      .catch((error) => {
        ensuredRuntime = undefined
        writeAppError("app.daemon.runtime_failed", error, {
          durationMs: Date.now() - startedAt,
          mode,
        })
        throw error
      })
  }
  return ensuredRuntime
}

/** Reads the app-bundled manifest, installs runtime files, and starts or updates the daemon service. */
async function ensureDaemonRuntimeInner() {
  const debug = getAppDebug("daemon.runtime")
  const daemon = resolveDaemonConnection()
  const mode = isDevelopmentRuntime() ? "development" : "packaged"
  debug("app.daemon.runtime.ensure_started", {
    daemonUrl: daemon.daemonUrl,
    mode,
  })

  if (mode === "development") {
    await ensureDevelopmentDaemonRuntime(daemon.daemonUrl)
    debug("app.daemon.runtime.development_ready", {
      daemonUrl: daemon.daemonUrl,
    })
    return { daemonUrl: daemon.daemonUrl }
  }

  const manifest = await readEmbeddedRuntimeManifest()
  const baseUrl = await resolveDaemonBaseUrl()
  const installedState = await readInstalledDaemonState()
  const preparedRuntime = await prepareDaemonRuntime(manifest)
  const daemonResponded = await pingDaemon(daemon.daemonUrl).catch(() => false)
  const canReuseRuntime =
    installedState?.runtimeHash === preparedRuntime.runtimeHash && daemonResponded

  debug("app.daemon.runtime.reuse_resolved", {
    daemonResponded,
    installedRuntimeHash: installedState?.runtimeHash ?? null,
    runtimeHash: preparedRuntime.runtimeHash,
    reusable: canReuseRuntime,
  })
  if (canReuseRuntime) {
    return { daemonUrl: daemon.daemonUrl }
  }

  const installStartedAt = Date.now()
  debug("app.daemon.runtime.install_started", {
    platform: process.platform,
    runtimeHash: preparedRuntime.runtimeHash,
  })
  if (process.platform === "win32") {
    await installWindowsDaemonStartup(preparedRuntime, baseUrl, daemon.port)
  } else {
    await installUnixDaemonService(manifest, preparedRuntime, baseUrl, daemon.port)
  }

  await waitForDaemonReady(daemon.daemonUrl)
  await writeInstalledDaemonState({ runtimeHash: preparedRuntime.runtimeHash })
  debug("app.daemon.runtime.install_completed", {
    durationMs: Date.now() - installStartedAt,
    platform: process.platform,
    runtimeHash: preparedRuntime.runtimeHash,
  })

  return { daemonUrl: daemon.daemonUrl }
}

/** In development, reuse the separately watched daemon process instead of the app-bundled runtime. */
async function ensureDevelopmentDaemonRuntime(daemonUrl: string) {
  try {
    await waitForDaemonReady(daemonUrl, 5_000)
  } catch {
    throw new Error(
      "Development mode expects a running Goddard daemon. Start `pnpm run dev` from the workspace root, or launch `core/daemon` before starting the app.",
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

  await writeAppBunLaunchers(manifest, installDir)

  return {
    daemonRootDir: installDir,
    daemonExecutablePath: join(installDir, manifest.daemon.executablePath),
    agentBinDir: join(installDir, manifest.daemon.agentBinDir),
    ...resolveInstalledNativeRuntimePaths(manifest, installDir),
    runtimeHash: manifest.daemon.runtimeHash,
  } satisfies PreparedDaemonRuntime
}

/** Points lightweight packaged launchers at the current app-bundled Bun executable. */
async function writeAppBunLaunchers(manifest: EmbeddedRuntimeManifest, installDir: string) {
  await Promise.all(
    (manifest.daemon.sharedBunLauncherPaths ?? []).map(async (relativeLauncherPath) => {
      const launcherPath = join(installDir, relativeLauncherPath)
      const launcher = bindBunRuntimeLauncher(
        await readFile(launcherPath, "utf8"),
        process.execPath,
      )
      if (!launcher) {
        return
      }

      await writeFile(launcherPath, launcher, "utf8")
      await chmod(launcherPath, 0o755)
    }),
  )
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
    ...createDaemonRunArgs({
      runtime,
      baseUrl,
      daemonPort,
      dataProfile,
    }),
  )

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
  const daemonArgs = createDaemonRunArgs({
    runtime,
    baseUrl,
    daemonPort,
    dataProfile,
  })

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
  const debug = getAppDebug("daemon.runtime")
  const startedAt = Date.now()
  const deadline = Date.now() + timeoutMs
  let attempt = 0
  debug("app.daemon.runtime.readiness_wait_started", {
    daemonUrl,
    timeoutMs,
  })

  while (Date.now() < deadline) {
    attempt += 1
    if (await pingDaemon(daemonUrl).catch(() => false)) {
      debug("app.daemon.runtime.readiness_succeeded", {
        attempt,
        daemonUrl,
        durationMs: Date.now() - startedAt,
      })
      return
    }

    await Bun.sleep(250)
  }

  debug("app.daemon.runtime.readiness_timed_out", {
    attempt,
    daemonUrl,
    durationMs: Date.now() - startedAt,
    timeoutMs,
  })
  throw new Error(`Timed out waiting for the Goddard daemon at ${daemonUrl}`)
}

/** Sends a daemon health check request and returns whether the daemon answered successfully. */
async function pingDaemon(daemonUrl: string) {
  const client = createDaemonIpcClient({ daemonUrl })
  const response = await client.daemon.health()
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
