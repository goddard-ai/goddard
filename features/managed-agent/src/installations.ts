import { createHash } from "node:crypto"
import { mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { getGoddardGlobalDir } from "@goddard-ai/paths/node"
import {
  agentBinaryPlatforms,
  type AgentBinaryPlatform,
} from "@goddard-ai/schema/agent-distribution"
import {
  binaryInstallMarkerFileName,
  installBinaryTargetPayload,
  type AdapterCatalogEntry,
} from "acp-client/node"
import { z } from "zod"

import type { ManagedAgentInstallationState } from "./schema.ts"

function getAdapterInstallationsPath() {
  return join(getGoddardGlobalDir(), "adapter-installations.json")
}

const AdapterInstallationsFile = z
  .strictObject({
    installedAdapterIds: z.array(z.string()).default([]),
  })
  .default({ installedAdapterIds: [] })

type AdapterInstallationsFile = z.infer<typeof AdapterInstallationsFile>

function toAgentBinaryPlatform(
  platform: NodeJS.Platform,
  arch: string,
): AgentBinaryPlatform | null {
  const normalizedPlatform =
    platform === "win32"
      ? "windows"
      : platform === "darwin"
        ? "darwin"
        : platform === "linux"
          ? "linux"
          : null
  const normalizedArch = arch === "arm64" ? "aarch64" : arch === "x64" ? "x86_64" : null
  if (!normalizedPlatform || !normalizedArch) {
    return null
  }

  const key = `${normalizedPlatform}-${normalizedArch}`
  return agentBinaryPlatforms.includes(key as AgentBinaryPlatform)
    ? (key as AgentBinaryPlatform)
    : null
}

function resolveAdapterInstallMethod(adapter: AdapterCatalogEntry) {
  const platformKey = toAgentBinaryPlatform(process.platform, process.arch)
  const binaryTarget = platformKey ? adapter.distribution.binary?.[platformKey] : undefined

  if (binaryTarget && platformKey) {
    return {
      type: "binary" as const,
      platformKey,
      target: binaryTarget,
    }
  }

  if (adapter.distribution.npx) {
    return {
      type: "npx" as const,
      packageName: adapter.distribution.npx.package,
    }
  }

  if (adapter.distribution.uvx) {
    return {
      type: "uvx" as const,
      packageName: adapter.distribution.uvx.package,
    }
  }

  return null
}

function getBinaryInstallDir(
  adapter: AdapterCatalogEntry,
  method: Extract<NonNullable<ReturnType<typeof resolveAdapterInstallMethod>>, { type: "binary" }>,
) {
  const archiveHash = createHash("sha256").update(method.target.archive).digest("hex").slice(0, 12)

  return join(
    getGoddardGlobalDir(),
    "binaries",
    `${adapter.id}-${adapter.version}-${method.platformKey}-${archiveHash}`,
  )
}

async function pathExists(path: string) {
  try {
    await readFile(path)
    return true
  } catch (error) {
    if (typeof error === "object" && error && "code" in error && error.code === "ENOENT") {
      return false
    }

    throw error
  }
}

async function readAdapterInstallationsFile(): Promise<AdapterInstallationsFile> {
  try {
    return AdapterInstallationsFile.parse(
      JSON.parse(await readFile(getAdapterInstallationsPath(), "utf8")),
    )
  } catch (error) {
    if (typeof error === "object" && error && "code" in error && error.code === "ENOENT") {
      return AdapterInstallationsFile.parse(undefined)
    }

    throw error
  }
}

async function writeAdapterInstallationsFile(file: AdapterInstallationsFile) {
  await mkdir(getGoddardGlobalDir(), { recursive: true })
  await writeFile(
    getAdapterInstallationsPath(),
    `${JSON.stringify(AdapterInstallationsFile.parse(file), null, 2)}\n`,
    "utf8",
  )
}

async function readInstalledAdapterIds() {
  return new Set((await readAdapterInstallationsFile()).installedAdapterIds)
}

async function writeInstalledAdapterIds(adapterIds: Set<string>) {
  await writeAdapterInstallationsFile({
    installedAdapterIds: [...adapterIds].sort(),
  })
}

async function isBinaryInstalled(
  adapter: AdapterCatalogEntry,
  method: Extract<NonNullable<ReturnType<typeof resolveAdapterInstallMethod>>, { type: "binary" }>,
) {
  return await pathExists(join(getBinaryInstallDir(adapter, method), binaryInstallMarkerFileName))
}

async function removeBinaryInstalls(adapterId: string) {
  const binariesDir = join(getGoddardGlobalDir(), "binaries")

  try {
    const entries = await readdir(binariesDir, { withFileTypes: true })
    await Promise.all(
      entries
        .filter((entry) => entry.isDirectory() && entry.name.startsWith(`${adapterId}-`))
        .map((entry) => rm(join(binariesDir, entry.name), { recursive: true, force: true })),
    )
  } catch (error) {
    if (typeof error === "object" && error && "code" in error && error.code === "ENOENT") {
      return
    }

    throw error
  }
}

export async function getManagedAgentInstallationState(
  adapter: AdapterCatalogEntry,
  installedAdapterIds: Set<string>,
): Promise<ManagedAgentInstallationState> {
  if (adapter.source === "config") {
    return {
      managedAgentId: adapter.id,
      installable: false,
      installed: true,
      method: "config",
    }
  }

  const method = resolveAdapterInstallMethod(adapter)
  const binaryInstalled =
    method?.type === "binary" ? await isBinaryInstalled(adapter, method) : false

  return {
    managedAgentId: adapter.id,
    installable: method !== null,
    installed: installedAdapterIds.has(adapter.id) || binaryInstalled,
    method: method?.type ?? "unsupported",
  }
}

export async function getManagedAgentInstallationStates(adapters: AdapterCatalogEntry[]) {
  const installedAdapterIds = await readInstalledAdapterIds()

  return await Promise.all(
    adapters.map((adapter) => getManagedAgentInstallationState(adapter, installedAdapterIds)),
  )
}

export async function installManagedAgent(adapter: AdapterCatalogEntry) {
  if (adapter.source === "config") {
    return await getManagedAgentInstallationState(adapter, await readInstalledAdapterIds())
  }

  const method = resolveAdapterInstallMethod(adapter)
  if (!method) {
    throw new Error(`Adapter ${adapter.id} does not support installation on this platform.`)
  }

  if (method.type === "binary") {
    const installDir = getBinaryInstallDir(adapter, method)
    await mkdir(join(getGoddardGlobalDir(), "binaries"), { recursive: true })
    await rm(installDir, { recursive: true, force: true })
    await installBinaryTargetPayload({
      archiveUrl: method.target.archive,
      cmd: method.target.cmd,
      installDir,
    })
    await writeFile(
      join(installDir, binaryInstallMarkerFileName),
      `${method.target.archive}\n`,
      "utf8",
    )
  }

  const installedAdapterIds = await readInstalledAdapterIds()
  installedAdapterIds.add(adapter.id)
  await writeInstalledAdapterIds(installedAdapterIds)

  return await getManagedAgentInstallationState(adapter, installedAdapterIds)
}

export async function uninstallManagedAgent(managedAgentId: string) {
  const installedAdapterIds = await readInstalledAdapterIds()
  installedAdapterIds.delete(managedAgentId)
  await writeInstalledAdapterIds(installedAdapterIds)
  await removeBinaryInstalls(managedAgentId)
}
