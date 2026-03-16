import { createJiti } from "@mariozechner/jiti"
import { join } from "node:path"
import { readFile, mkdir, writeFile } from "node:fs/promises"
import { createLoop, type AgentLoopHandler, type LoopRuntimeConfig } from "@goddard-ai/loop"
import { LoopStorage, fileExists, getGlobalConfigPath, getLoopDir } from "@goddard-ai/storage"
import type { NodeGoddardLoopRunOverrides } from "./index.ts"

const DEFAULT_RUNTIME_CONFIG = {
  agent: "anthropic/claude-3-7-sonnet-20250219",
  systemPrompt: "Make one safe improvement. Reply SUMMARY|DONE when finished.",
  strategy: "",
  mcpServers: [] as any[],
}

type LegacyLoopConfig = Record<string, unknown>
type LoopConfigModule = LegacyLoopConfig | { default: LegacyLoopConfig }

export async function loadLoopConfig(
  cwd: string = process.cwd(),
  loopId: string = "default",
): Promise<{ config: LoopRuntimeConfig }> {
  const record = await LoopStorage.get(loopId)
  if (record) {
    const loopDir = getLoopDir(loopId)
    let fileConfig: Record<string, unknown> = {}
    try {
      const configJson = await readFile(join(loopDir, "config.json"), "utf8")
      fileConfig = JSON.parse(configJson)
    } catch {
      // Ignore if missing or invalid
    }

    let systemPrompt = DEFAULT_RUNTIME_CONFIG.systemPrompt
    const jiti = createJiti(loopDir)
    try {
      const promptModule = (await jiti.import(join(loopDir, "prompt.ts"))) as { nextPrompt?: () => unknown }
      if (typeof promptModule.nextPrompt === "function") {
        const computed = await promptModule.nextPrompt()
        if (typeof computed === "string" && computed.trim().length > 0) {
          systemPrompt = computed
        }
      }
    } catch {
      try {
        const promptMd = await readFile(join(loopDir, "prompt.md"), "utf8")
        if (promptMd.trim().length > 0) {
          systemPrompt = promptMd
        }
      } catch {
        // Ignore if both prompt.ts and prompt.md are missing
      }
    }

    return {
      config: {
        ...DEFAULT_RUNTIME_CONFIG,
        cwd,
        ...fileConfig,
        systemPrompt,
      } as LoopRuntimeConfig,
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

async function upsertStoredLoopConfig(loopId: string, config: LoopRuntimeConfig): Promise<void> {
  const loopDir = getLoopDir(loopId)
  await mkdir(loopDir, { recursive: true })

  const { systemPrompt, cwd: _cwd, ...restConfig } = config
  await writeFile(join(loopDir, "config.json"), JSON.stringify(restConfig, null, 2), "utf8")
  await writeFile(join(loopDir, "prompt.md"), systemPrompt, "utf8")

  const existing = await LoopStorage.get(loopId)
  if (!existing) {
    await LoopStorage.create({ id: loopId })
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
