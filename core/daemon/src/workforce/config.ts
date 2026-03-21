import type { WorkforceAgentConfig, WorkforceConfig } from "@goddard-ai/schema/workforce"
import { mkdir } from "node:fs/promises"
import { buildWorkforcePaths } from "./paths.js"

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && Array.isArray(value) === false
}

function assertAgentConfig(value: unknown, index: number): asserts value is WorkforceAgentConfig {
  if (!isRecord(value)) {
    throw new Error(`Invalid workforce agent at index ${index}`)
  }

  if (typeof value.id !== "string" || value.id.length === 0) {
    throw new Error(`Workforce agent ${index} must include a non-empty id`)
  }

  if (typeof value.name !== "string" || value.name.length === 0) {
    throw new Error(`Workforce agent ${index} must include a non-empty name`)
  }

  if (value.role !== "root" && value.role !== "domain") {
    throw new Error(`Workforce agent ${index} has an invalid role`)
  }

  if (typeof value.cwd !== "string" || value.cwd.length === 0) {
    throw new Error(`Workforce agent ${index} must include a non-empty cwd`)
  }

  if (
    Array.isArray(value.owns) === false ||
    value.owns.some((entry) => typeof entry !== "string" || entry.length === 0)
  ) {
    throw new Error(`Workforce agent ${index} must include non-empty owned paths`)
  }
}

export async function readWorkforceConfig(rootDir: string): Promise<WorkforceConfig> {
  const paths = buildWorkforcePaths(rootDir)
  const parsed = JSON.parse(await Bun.file(paths.configPath).text()) as unknown

  if (!isRecord(parsed) || parsed.version !== 1) {
    throw new Error(`Invalid workforce config at ${paths.configPath}`)
  }

  if (
    (typeof parsed.defaultAgent !== "string" && isRecord(parsed.defaultAgent) === false) ||
    typeof parsed.rootAgentId !== "string" ||
    Array.isArray(parsed.agents) === false
  ) {
    throw new Error(`Invalid workforce config at ${paths.configPath}`)
  }

  parsed.agents.forEach((agent, index) => {
    assertAgentConfig(agent, index)
  })

  if (parsed.agents.some((agent) => agent.id === parsed.rootAgentId) === false) {
    throw new Error(`Workforce config at ${paths.configPath} must include the root agent`)
  }

  return parsed as unknown as WorkforceConfig
}

export async function ensureWorkforceFiles(rootDir: string): Promise<void> {
  const paths = buildWorkforcePaths(rootDir)
  await mkdir(paths.goddardDir, { recursive: true })

  try {
    await Bun.file(paths.ledgerPath).text()
  } catch {
    await Bun.write(paths.ledgerPath, "")
  }
}
