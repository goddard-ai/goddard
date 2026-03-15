import { createJiti } from "@mariozechner/jiti"
import { mkdir, writeFile } from "node:fs/promises"
import { homedir } from "node:os"
import { dirname, join } from "node:path"
import { createLoop, type AgentLoopHandler, type LoopRuntimeConfig } from "@goddard-ai/loop"
import { LoopStorage, fileExists, getGlobalConfigPath } from "@goddard-ai/storage"
import type { NodeGoddardLoopRunOverrides } from "./index.ts"

const DEFAULT_LOOP_CONFIG_TEMPLATE = `import { defineConfig } from "@goddard-ai/config"

export default defineConfig({})
`

const DEFAULT_RUNTIME_CONFIG = {
  agent: "anthropic/claude-3-7-sonnet-20250219",
  systemPrompt: "Make one safe improvement. Reply SUMMARY|DONE when finished.",
  strategy: "",
  mcpServers: [] as any[],
}

type LegacyLoopConfig = Record<string, unknown>
type LoopConfigModule = LegacyLoopConfig | { default: LegacyLoopConfig }

type LegacySystemdConfig = {
  restartSec?: number
  nice?: number
  user?: string
  workingDir?: string
  environment?: Record<string, string | undefined>
}

export async function initLoopConfig(options: { global?: boolean }): Promise<{ path: string }> {
  const targetPath = options.global ? getGlobalConfigPath() : getLocalConfigPath(process.cwd())

  if (await fileExists(targetPath)) {
    throw new Error(`Config file already exists at ${targetPath}`)
  }

  await mkdir(dirname(targetPath), { recursive: true })
  await writeFile(targetPath, DEFAULT_LOOP_CONFIG_TEMPLATE, "utf-8")

  return { path: targetPath }
}

export async function loadLoopConfig(
  cwd: string = process.cwd(),
  loopId: string = "default",
): Promise<{ config: LoopRuntimeConfig }> {
  const record = await LoopStorage.get(loopId)
  if (record) {
    return {
      config: {
        agent: record.agent,
        cwd: record.cwd,
        systemPrompt: record.systemPrompt,
        strategy: record.strategy ?? undefined,
        mcpServers: record.mcpServers ?? [],
      },
    }
  }

  const legacy = await loadLegacyLoopConfig(cwd)
  const migratedConfig = legacy ? toRuntimeConfig(legacy.config, cwd) : null

  if (migratedConfig) {
    await upsertStoredLoopConfig(loopId, migratedConfig)
    return { config: migratedConfig }
  }

  return {
    config: {
      ...DEFAULT_RUNTIME_CONFIG,
      cwd,
    },
  }
}

export async function runAgentLoop(
  cwd: string = process.cwd(),
  _overrides?: NodeGoddardLoopRunOverrides,
  _handler?: AgentLoopHandler,
): Promise<void> {
  await runLoop(cwd)
}

export async function runLoop(
  cwd: string = process.cwd(),
  loopId: string = "default",
  deps?: { createLoopRuntime?: typeof createLoop },
): Promise<void> {
  const { config } = await loadLoopConfig(cwd, loopId)
  const runtime = deps?.createLoopRuntime ?? createLoop
  const loop = runtime(config)
  await loop.start()
}

export async function generateLoopSystemdService(
  cwd: string = process.cwd(),
  options: { global?: boolean; user?: string },
): Promise<{ path: string }> {
  const legacy = await loadLegacyLoopConfig(cwd, { global: options.global })
  const systemd = getSystemdConfig(legacy?.config)
  const targetRoot = options.global ? homedir() : cwd
  const outputPath = join(targetRoot, "systemd", "goddard.service")

  const user = systemd?.user ?? options.user ?? process.env.USER ?? "root"
  const workingDir = systemd?.workingDir ?? cwd
  const restartSec = systemd?.restartSec ?? 10
  const nice = systemd?.nice ?? 10
  const environment = renderSystemdEnvironment(systemd?.environment)

  const service = `[Unit]\nDescription=Goddard Autonomous Agent Loop\nAfter=network.target\n\n[Service]\nType=simple\nUser=${user}\nWorkingDirectory=${workingDir}\nExecStart=goddard loop run\nRestart=always\nRestartSec=${restartSec}\nNice=${nice}\n${environment}[Install]\nWantedBy=multi-user.target\n`

  await mkdir(dirname(outputPath), { recursive: true })
  await writeFile(outputPath, service, "utf-8")

  return { path: outputPath }
}

async function loadLegacyLoopConfig(
  cwd: string,
  options?: { global?: boolean },
): Promise<{ config: LegacyLoopConfig; path: string } | null> {
  let configPath: string | null = null

  if (options?.global !== undefined) {
    configPath = options.global ? getGlobalConfigPath() : getLocalConfigPath(cwd)
    if (!(await fileExists(configPath))) {
      return null
    }
  } else {
    const localPath = getLocalConfigPath(cwd)
    if (await fileExists(localPath)) {
      configPath = localPath
    } else {
      const globalPath = getGlobalConfigPath()
      if (await fileExists(globalPath)) {
        configPath = globalPath
      }
    }
  }

  if (!configPath) {
    return null
  }

  const jiti = createJiti(cwd)
  const module = (await jiti.import(configPath)) as LoopConfigModule
  const config = ("default" in module ? module.default : module) as LegacyLoopConfig

  if (!config || typeof config !== "object") {
    throw new Error("Config file must export a default configuration object.")
  }

  return { config, path: configPath }
}

function getLocalConfigPath(cwd: string): string {
  return join(cwd, ".goddard", "config.ts")
}

function quoteSystemdValue(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`
}

function renderSystemdEnvironment(environment?: Record<string, string | undefined>): string {
  if (!environment) {
    return ""
  }

  const lines = Object.entries(environment)
    .filter(([, value]) => value !== undefined)
    .map(([key, value]) => `Environment=${key}=${quoteSystemdValue(value as string)}`)

  return lines.length > 0 ? `${lines.join("\n")}\n` : ""
}

function getSystemdConfig(config?: LegacyLoopConfig): LegacySystemdConfig | undefined {
  const systemd = asRecord(config?.systemd)
  if (!systemd) {
    return undefined
  }

  const environment = asRecord(systemd.environment)

  return {
    restartSec: typeof systemd.restartSec === "number" ? systemd.restartSec : undefined,
    nice: typeof systemd.nice === "number" ? systemd.nice : undefined,
    user: typeof systemd.user === "string" ? systemd.user : undefined,
    workingDir: typeof systemd.workingDir === "string" ? systemd.workingDir : undefined,
    environment: environment
      ? Object.fromEntries(
          Object.entries(environment).map(([key, value]) => [
            key,
            typeof value === "string" ? value : undefined,
          ]),
        )
      : undefined,
  }
}

async function upsertStoredLoopConfig(loopId: string, config: LoopRuntimeConfig): Promise<void> {
  const data = {
    agent: config.agent,
    systemPrompt: config.systemPrompt,
    strategy: config.strategy ?? "",
    displayName: loopId,
    cwd: config.cwd,
    mcpServers: config.mcpServers ?? [],
    gitRemote: "origin",
  }

  const existing = await LoopStorage.get(loopId)
  if (existing) {
    await LoopStorage.update(loopId, data)
  } else {
    await LoopStorage.create({
      id: loopId,
      ...data,
    })
  }
}

function toRuntimeConfig(config: LegacyLoopConfig, cwd: string): LoopRuntimeConfig | null {
  const directAgent = typeof config.agent === "string" ? config.agent : null
  const directCwd = typeof config.cwd === "string" ? config.cwd : null
  const directSystemPrompt = typeof config.systemPrompt === "string" ? config.systemPrompt : null

  if (directAgent && directCwd && directSystemPrompt) {
    return {
      agent: directAgent,
      cwd: directCwd,
      systemPrompt: directSystemPrompt,
      strategy: typeof config.strategy === "string" ? config.strategy : undefined,
      mcpServers: Array.isArray(config.mcpServers) ? config.mcpServers : [],
    }
  }

  const legacyAgent = asRecord(config.agent)
  const legacyModel = typeof legacyAgent?.model === "string" ? legacyAgent.model : null

  if (!legacyModel) {
    return null
  }

  let systemPrompt = DEFAULT_RUNTIME_CONFIG.systemPrompt
  if (directSystemPrompt) {
    systemPrompt = directSystemPrompt
  } else if (typeof config.nextPrompt === "function") {
    try {
      const nextPrompt = config.nextPrompt as () => unknown
      const computedPrompt = nextPrompt()
      if (typeof computedPrompt === "string" && computedPrompt.trim().length > 0) {
        systemPrompt = computedPrompt
      }
    } catch {
      // Fall back to the default runtime prompt.
    }
  }

  return {
    agent: legacyModel,
    cwd: typeof legacyAgent?.projectDir === "string" ? legacyAgent.projectDir : cwd,
    systemPrompt,
    strategy: typeof config.strategy === "string" ? config.strategy : undefined,
    mcpServers: Array.isArray(config.mcpServers) ? config.mcpServers : [],
  }
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" ? (value as Record<string, unknown>) : null
}
