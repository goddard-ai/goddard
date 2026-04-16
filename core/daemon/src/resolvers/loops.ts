import { mergeLoopConfigLayers, resolveDefaultAgent } from "@goddard-ai/config"
import { ResolvedLoopConfig, type LoopConfig } from "@goddard-ai/schema/config"
import type { CreateSessionRequest, StartLoopRequest } from "@goddard-ai/schema/daemon"
import { existsSync } from "node:fs"
import { join, resolve } from "node:path"

import type { RootConfigProvider } from "./config.ts"
import { readCurrentRootConfig, readLoopConfig } from "./config.ts"

type RequiredObject<T> = Required<Exclude<T, undefined>>

/** Fully resolved daemon-owned loop runtime config after local package resolution. */
export type ResolvedLoopStartRequest = {
  rootDir: string
  loopName: string
  promptModulePath: string
  session: Omit<CreateSessionRequest, "initialPrompt" | "oneShot">
  rateLimits: RequiredObject<StartLoopRequest["rateLimits"]>
  retries: RequiredObject<StartLoopRequest["retries"]>
}

/** A resolved named loop package with merged persisted config and prompt module path. */
export type ResolvedLoop = {
  config: LoopConfig
  path: string
  promptModulePath: string
}

/** Default retry config applied when persisted loop config omits explicit values. */
const DEFAULT_LOOP_RETRIES = {
  maxAttempts: 1,
  initialDelayMs: 500,
  maxDelayMs: 5_000,
  backoffFactor: 2,
  jitterRatio: 0.2,
}

/** Resolves one merged loop config document into the fully required runtime contract. */
function resolveLoopRuntimeConfig(
  config: LoopConfig,
  rootDir: string,
  overrides?: Pick<StartLoopRequest, "session" | "rateLimits" | "retries">,
) {
  const sessionConfig = config.session ?? {}
  const sessionOverrides = overrides?.session ?? {}
  const rateLimitConfig = config.rateLimits ?? {}
  const rateLimitOverrides = overrides?.rateLimits ?? {}
  const retryOverrides = overrides?.retries ?? {}

  return ResolvedLoopConfig.parse({
    session: {
      agent: sessionOverrides.agent ?? sessionConfig.agent,
      cwd: resolve(sessionOverrides.cwd ?? rootDir),
      mcpServers: sessionOverrides.mcpServers ?? sessionConfig.mcpServers ?? [],
      env: sessionOverrides.env ?? sessionConfig.env,
      systemPrompt: sessionOverrides.systemPrompt,
      repository: sessionOverrides.repository,
      prNumber: sessionOverrides.prNumber,
      metadata: sessionOverrides.metadata,
    },
    rateLimits: {
      cycleDelay: rateLimitOverrides.cycleDelay ?? rateLimitConfig.cycleDelay,
      maxOpsPerMinute: rateLimitOverrides.maxOpsPerMinute ?? rateLimitConfig.maxOpsPerMinute,
      maxCyclesBeforePause:
        rateLimitOverrides.maxCyclesBeforePause ?? rateLimitConfig.maxCyclesBeforePause,
    },
    retries: {
      maxAttempts:
        retryOverrides.maxAttempts ??
        config.retries?.maxAttempts ??
        DEFAULT_LOOP_RETRIES.maxAttempts,
      initialDelayMs:
        retryOverrides.initialDelayMs ??
        config.retries?.initialDelayMs ??
        DEFAULT_LOOP_RETRIES.initialDelayMs,
      maxDelayMs:
        retryOverrides.maxDelayMs ?? config.retries?.maxDelayMs ?? DEFAULT_LOOP_RETRIES.maxDelayMs,
      backoffFactor:
        retryOverrides.backoffFactor ??
        config.retries?.backoffFactor ??
        DEFAULT_LOOP_RETRIES.backoffFactor,
      jitterRatio:
        retryOverrides.jitterRatio ??
        config.retries?.jitterRatio ??
        DEFAULT_LOOP_RETRIES.jitterRatio,
    },
  })
}

/** Loads one packaged loop from disk and verifies the required prompt/config files exist. */
async function loadPackagedLoop(path: string): Promise<ResolvedLoop> {
  const promptFilePath = join(path, "prompt.js")
  const promptMarkdownPath = join(path, "prompt.md")
  const configPath = join(path, "config.json")

  if (existsSync(promptMarkdownPath)) {
    throw new Error(
      `Loop directory "${path}" must not contain prompt.md when prompt.js is required.`,
    )
  }

  if (!existsSync(promptFilePath)) {
    throw new Error(`Loop directory "${path}" must include a prompt.js file.`)
  }

  if (!existsSync(configPath)) {
    throw new Error(`Loop directory "${path}" must include a config.json file.`)
  }

  return {
    config: (await readLoopConfig(configPath)) ?? {},
    path,
    promptModulePath: promptFilePath,
  }
}

/** Resolves one named loop package from a specific global or local `.goddard` root. */
async function resolveLoopFromRoot(
  loopName: string,
  goddardRoot: string,
): Promise<ResolvedLoop | null> {
  const promptOnlyPath = join(goddardRoot, "loops", `${loopName}.md`)
  const folderPath = join(goddardRoot, "loops", loopName)

  if (existsSync(promptOnlyPath)) {
    throw new Error(
      `Loop "${loopName}" under "${goddardRoot}" is invalid. Runnable loops must be packaged with prompt.js and config.json.`,
    )
  }

  if (!existsSync(folderPath)) {
    return null
  }

  return loadPackagedLoop(folderPath)
}

/** Resolves one named packaged loop from local or global config roots. */
export async function resolveNamedLoop(
  loopName: string,
  rootDir: string,
  rootConfigProvider?: RootConfigProvider,
): Promise<ResolvedLoop> {
  const resolvedRootDir = resolve(rootDir)
  const { config, globalRoot, localRoot } = await readCurrentRootConfig(
    resolvedRootDir,
    rootConfigProvider,
  )
  const localLoop = await resolveLoopFromRoot(loopName, localRoot)
  const globalLoop = localLoop ? null : await resolveLoopFromRoot(loopName, globalRoot)
  const loop = localLoop ?? globalLoop

  if (!loop) {
    throw new Error(
      `Loop "${loopName}" not found in local or global configuration (.goddard/loops/<name>/).`,
    )
  }

  const mergedConfig = mergeLoopConfigLayers(config.loops, loop.config)
  const sessionConfig = mergedConfig.session ?? {}

  sessionConfig.agent = sessionConfig.agent ?? (await resolveDefaultAgent(config, "loops"))
  mergedConfig.session = sessionConfig

  return {
    ...loop,
    config: mergeLoopConfigLayers(
      config.session ? { session: config.session } : undefined,
      mergedConfig,
    ),
  }
}

/** Resolves one public loop-start request into the runtime config owned by the daemon. */
export async function resolveNamedLoopStartRequest(
  input: StartLoopRequest,
  rootConfigProvider?: RootConfigProvider,
): Promise<ResolvedLoopStartRequest> {
  const resolvedRootDir = resolve(input.rootDir)
  const loop = await resolveNamedLoop(input.loopName, resolvedRootDir, rootConfigProvider)
  const resolvedConfig = resolveLoopRuntimeConfig(loop.config, resolvedRootDir, input)

  return {
    rootDir: resolvedRootDir,
    loopName: input.loopName,
    promptModulePath: resolve(loop.promptModulePath),
    session: resolvedConfig.session as ResolvedLoopStartRequest["session"],
    rateLimits: resolvedConfig.rateLimits,
    retries: resolvedConfig.retries,
  }
}
