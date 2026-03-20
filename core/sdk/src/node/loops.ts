import { mergeLoopConfigLayers } from "@goddard-ai/config"
import {
  ResolvedLoopConfig,
  type GoddardLoopConfigDocument,
  type ResolvedGoddardLoopConfigDocument,
} from "@goddard-ai/schema/config"
import type {
  DaemonLoop,
  DaemonLoopStatus,
  StartDaemonLoopRequest,
} from "@goddard-ai/schema/daemon"
import { existsSync } from "node:fs"
import { join, resolve } from "node:path"
import { readLoopConfig, readMergedRootConfig } from "./config.js"
import {
  type LoopClientOptions,
  getDaemonLoop,
  listDaemonLoops,
  shutdownDaemonLoop,
  startDaemonLoop,
} from "../daemon/loops.js"

/** Runtime overrides accepted when starting one packaged daemon-owned loop. */
export type AgentLoopRuntimeOverrides = {
  session?: Partial<StartDaemonLoopRequest["session"]>
  rateLimits?: Partial<StartDaemonLoopRequest["rateLimits"]>
  retries?: Partial<StartDaemonLoopRequest["retries"]>
}

/** A resolved named loop package with merged persisted config and a prompt module path. */
export type ResolvedAgentLoop = {
  config: GoddardLoopConfigDocument
  path: string
  promptModulePath: string
}

/** Default retry config applied when persisted loop config omits explicit values. */
const DEFAULT_LOOP_RETRIES: StartDaemonLoopRequest["retries"] = {
  maxAttempts: 1,
  initialDelayMs: 500,
  maxDelayMs: 5_000,
  backoffFactor: 2,
  jitterRatio: 0.2,
}

/** Splits caller overrides into the persisted loop config layers accepted by the resolver. */
function toPersistedLoopOverrides(
  overrides: AgentLoopRuntimeOverrides | undefined,
): GoddardLoopConfigDocument | undefined {
  if (!overrides) {
    return undefined
  }

  return {
    session: overrides.session as GoddardLoopConfigDocument["session"],
    rateLimits: overrides.rateLimits as GoddardLoopConfigDocument["rateLimits"],
    retries: overrides.retries as GoddardLoopConfigDocument["retries"],
  }
}

/** Resolves one merged loop config document into the fully required runtime contract. */
function resolveLoopConfig(config: GoddardLoopConfigDocument): ResolvedGoddardLoopConfigDocument {
  return ResolvedLoopConfig.parse({
    session: config.session,
    rateLimits: config.rateLimits,
    retries: {
      ...DEFAULT_LOOP_RETRIES,
      ...config.retries,
    },
  })
}

/** Loads one packaged loop from disk and verifies the required prompt/config files exist. */
async function loadPackagedLoop(path: string): Promise<ResolvedAgentLoop> {
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

/** Resolves one named loop package from a specific global or local Goddard root. */
async function resolveLoopFromRoot(
  loopName: string,
  goddardRoot: string,
): Promise<ResolvedAgentLoop | null> {
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

/** Resolves a named loop package from local or global config roots. */
export async function resolveLoop(
  loopName: string,
  cwd: string = process.cwd(),
): Promise<ResolvedAgentLoop> {
  const { config, globalRoot, localRoot } = await readMergedRootConfig(cwd)
  const localLoop = await resolveLoopFromRoot(loopName, localRoot)
  const globalLoop = localLoop ? null : await resolveLoopFromRoot(loopName, globalRoot)
  const loop = localLoop ?? globalLoop

  if (!loop) {
    throw new Error(
      `Loop "${loopName}" not found in local or global configuration (.goddard/loops/<name>/).`,
    )
  }

  return {
    ...loop,
    config: mergeLoopConfigLayers(config.loops, loop.config),
  }
}

/** Builds the daemon start payload for one resolved packaged loop and caller overrides. */
export function buildLoopStartRequest(
  loopName: string,
  rootDir: string,
  loop: ResolvedAgentLoop,
  overrides?: AgentLoopRuntimeOverrides,
): StartDaemonLoopRequest {
  const resolvedConfig = resolveLoopConfig(
    mergeLoopConfigLayers(loop.config, toPersistedLoopOverrides(overrides)),
  )

  return {
    rootDir: resolve(rootDir),
    loopName,
    promptModulePath: resolve(loop.promptModulePath),
    session: resolvedConfig.session as StartDaemonLoopRequest["session"],
    rateLimits: resolvedConfig.rateLimits,
    retries: resolvedConfig.retries,
  }
}

/** Resolves a named packaged loop and starts it as a daemon-owned runtime. */
export async function startNamedLoop(
  loopName: string,
  overrides?: AgentLoopRuntimeOverrides,
  options?: LoopClientOptions,
): Promise<DaemonLoop> {
  const rootDir = overrides?.session?.cwd ?? process.cwd()
  return startDaemonLoop(
    buildLoopStartRequest(loopName, rootDir, await resolveLoop(loopName, rootDir), overrides),
    options,
  )
}

/** Fetches one daemon-owned loop runtime for the given repository root and loop name. */
export async function getLoop(
  rootDir: string,
  loopName: string,
  options?: LoopClientOptions,
): Promise<DaemonLoop> {
  return getDaemonLoop(resolve(rootDir), loopName, options)
}

/** Lists all daemon-owned loop runtimes currently running in the daemon. */
export async function listLoops(options?: LoopClientOptions): Promise<DaemonLoopStatus[]> {
  return listDaemonLoops(options)
}

/** Stops one daemon-owned loop runtime for the given repository root and loop name. */
export async function stopLoop(
  rootDir: string,
  loopName: string,
  options?: LoopClientOptions,
): Promise<boolean> {
  return shutdownDaemonLoop(resolve(rootDir), loopName, options)
}
