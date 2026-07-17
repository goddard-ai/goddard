import { createHash } from "node:crypto"
import { constants as fsConstants } from "node:fs"
import { access, mkdir, mkdtemp, readdir, rename, rm, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { getGoddardGlobalDir } from "@goddard-ai/paths/node"
import {
  agentBinaryPlatforms,
  type AgentBinaryPlatform,
  type AgentBinaryTarget,
  type AgentDistribution,
} from "@goddard-ai/schema/agent-distribution"
import type { ManagedAgentsConfig } from "@goddard-ai/schema/config"
import {
  binaryInstallMarkerFileName,
  installBinaryTargetPayload,
  resolveInstalledBinaryCommand,
} from "acp-client/node"

import type { ManagedAgentInstallService, ManagedAgentProcessSpec } from "./install-service.ts"

type ResolvedBinaryTarget = {
  platformKey: AgentBinaryPlatform
  target: AgentBinaryTarget
}

export type ResolveManagedAgentLaunchProcessSpecInput = {
  readonly agent: string | AgentDistribution
  readonly registry?: Record<string, AgentDistribution>
  readonly managedAgents?: ManagedAgentsConfig
}

/** Resolves the command used for one session launch, honoring managed install policy. */
export async function resolveManagedAgentLaunchProcessSpec(
  managedAgent: ManagedAgentInstallService,
  input: ResolveManagedAgentLaunchProcessSpecInput,
): Promise<ManagedAgentProcessSpec> {
  if (shouldUseManagedInstall(input.agent, input.managedAgents)) {
    const processSpec = await managedAgent.resolveInstalledAgentProcessSpec({
      agent: input.agent,
      registry: input.registry,
      installIfMissing: true,
    })

    return {
      ...processSpec,
      args: [...processSpec.args],
    }
  }

  const agent = await managedAgent.resolveAgent({
    agent: input.agent,
    registry: input.registry,
  })

  return resolveUnmanagedAgentProcessSpec(agent)
}

function shouldUseManagedInstall(
  agent: string | AgentDistribution,
  managedAgents?: ManagedAgentsConfig,
) {
  const agentId = typeof agent === "string" ? agent : agent.id
  return managedAgents?.[agentId]?.install === "beforeUse"
}

/** Chooses the concrete command invocation for an unmanaged resolved agent distribution. */
export async function resolveUnmanagedAgentProcessSpec(
  agent: AgentDistribution,
): Promise<ManagedAgentProcessSpec> {
  const binaryTarget = resolveBinaryTarget(agent)
  if (binaryTarget) {
    return {
      cmd: await resolveBinaryCommand(agent, binaryTarget),
      args: binaryTarget.target.args ?? [],
      env: binaryTarget.target.env,
    }
  }

  if (agent.distribution.npx) {
    return {
      cmd: "npx",
      args: ["-y", agent.distribution.npx.package, ...(agent.distribution.npx.args ?? [])],
      env: agent.distribution.npx.env,
    }
  }

  if (agent.distribution.uvx) {
    return {
      cmd: "uvx",
      args: [agent.distribution.uvx.package, ...(agent.distribution.uvx.args ?? [])],
      env: agent.distribution.uvx.env,
    }
  }

  throw new Error(`Unsupported agent distribution for ${agent.id}`)
}

function resolveBinaryTarget(agent: AgentDistribution): ResolvedBinaryTarget | null {
  const platformKey = toAgentBinaryPlatform(process.platform, process.arch)
  if (!platformKey) {
    return null
  }

  const target = agent.distribution.binary?.[platformKey]
  if (!target) {
    return null
  }

  return {
    platformKey,
    target,
  }
}

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

async function resolveBinaryCommand(
  agent: AgentDistribution,
  binaryTarget: ResolvedBinaryTarget,
): Promise<string> {
  const installDir = getBinaryInstallDir(agent, binaryTarget)
  const installMarkerPath = join(installDir, binaryInstallMarkerFileName)

  if (!(await pathExists(installMarkerPath))) {
    await installBinaryArchive(
      agent.id,
      binaryTarget.target.archive,
      binaryTarget.target.cmd,
      installDir,
    )
  }

  await cleanupOtherAgentBinaryInstalls(agent.id, installDir)

  return await resolveInstalledBinaryCommand(installDir, binaryTarget.target.cmd)
}

function getBinaryInstallDir(agent: AgentDistribution, binaryTarget: ResolvedBinaryTarget): string {
  const archiveHash = createHash("sha256")
    .update(binaryTarget.target.archive)
    .digest("hex")
    .slice(0, 12)

  return join(
    getGoddardGlobalDir(),
    "binaries",
    `${agent.id}-${agent.version}-${binaryTarget.platformKey}-${archiveHash}`,
  )
}

async function installBinaryArchive(
  agentId: string,
  archiveUrl: string,
  cmd: string,
  installDir: string,
): Promise<void> {
  const binariesDir = join(getGoddardGlobalDir(), "binaries")
  await mkdir(binariesDir, { recursive: true })
  await rm(installDir, { recursive: true, force: true })

  const stagingParentDir = await mkdtemp(join(binariesDir, "install-"))
  const stagedInstallDir = join(stagingParentDir, "install")

  try {
    await installBinaryTargetPayload({
      archiveUrl,
      cmd,
      installDir: stagedInstallDir,
    })
    await writeFile(join(stagedInstallDir, binaryInstallMarkerFileName), `${archiveUrl}\n`, "utf8")
    await rename(stagedInstallDir, installDir)
    await cleanupOtherAgentBinaryInstalls(agentId, installDir)
  } finally {
    await rm(stagingParentDir, { recursive: true, force: true })
  }
}

async function cleanupOtherAgentBinaryInstalls(
  agentId: string,
  activeInstallDir: string,
): Promise<void> {
  const binariesDir = join(getGoddardGlobalDir(), "binaries")
  if (!(await pathExists(binariesDir))) {
    return
  }

  const agentPrefix = `${agentId}-`
  const installs = await readdir(binariesDir, { withFileTypes: true })

  await Promise.all(
    installs
      .filter(
        (entry) =>
          entry.isDirectory() &&
          entry.name.startsWith(agentPrefix) &&
          join(binariesDir, entry.name) !== activeInstallDir,
      )
      .map((entry) => rm(join(binariesDir, entry.name), { recursive: true, force: true })),
  )
}

async function pathExists(path: string): Promise<boolean> {
  try {
    await access(path, fsConstants.F_OK)
    return true
  } catch {
    return false
  }
}
