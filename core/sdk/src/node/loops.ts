import { mergeLoopConfigLayers } from "@goddard-ai/config"
import {
  ResolvedLoopConfig,
  type GoddardLoopConfigDocument,
  type ResolvedGoddardLoopConfigDocument,
} from "@goddard-ai/schema/config"
import type {
  AgentLoopHandler,
  AgentLoopParams,
  AgentLoopRetryConfig,
} from "@goddard-ai/schema/loop"
import exitHook from "exit-hook"
import { existsSync } from "node:fs"
import { join, resolve } from "node:path"
import type { AgentSession } from "../daemon/session/client-session.ts"
import { type RunAgentOptions } from "../daemon/session/client.ts"
import { runAgentLoop } from "../loop/run-agent-loop.ts"
import { readLoopConfig, readMergedRootConfig } from "./config.ts"

/** The JSON-safe retry fields that may be persisted or layered from runtime overrides. */
type PersistedLoopRetryConfig = Omit<AgentLoopRetryConfig, "retryableErrors">

/** Runtime overrides accepted when starting a named or ad hoc loop. */
export type AgentLoopRuntimeOverrides = {
  session?: Partial<AgentLoopParams["session"]>
  rateLimits?: Partial<AgentLoopParams["rateLimits"]>
  retries?: Partial<PersistedLoopRetryConfig> & {
    retryableErrors?: AgentLoopRetryConfig["retryableErrors"]
  }
}

/** A resolved named loop package with merged persisted config and a prompt module path. */
export type ResolvedAgentLoop = {
  config: GoddardLoopConfigDocument
  path: string
  promptModulePath: string
}

/** The input contract for ad hoc loops that supply a prompt module directly. */
export type AdHocLoopInput = AgentLoopRuntimeOverrides & {
  promptModulePath: string
}

const defaultRetryableErrors: AgentLoopRetryConfig["retryableErrors"] = () => false

const DEFAULT_LOOP_RETRIES: PersistedLoopRetryConfig = {
  maxAttempts: 1,
  initialDelayMs: 500,
  maxDelayMs: 5_000,
  backoffFactor: 2,
  jitterRatio: 0.2,
}

function isCallable(value: unknown): value is () => string {
  return typeof value === "function"
}

function splitLoopOverrides(overrides: AgentLoopRuntimeOverrides | undefined): {
  persistedLayer: GoddardLoopConfigDocument | undefined
  retryableErrors: AgentLoopRetryConfig["retryableErrors"] | undefined
} {
  if (!overrides) {
    return {
      persistedLayer: undefined,
      retryableErrors: undefined,
    }
  }

  const { retries, ...rest } = overrides
  const { retryableErrors, ...persistedRetries } = retries ?? {}

  return {
    persistedLayer: {
      session: rest.session as GoddardLoopConfigDocument["session"],
      rateLimits: rest.rateLimits as GoddardLoopConfigDocument["rateLimits"],
      retries: retries ? persistedRetries : undefined,
    },
    retryableErrors,
  }
}

async function importNextPrompt(promptModulePath: string): Promise<() => string> {
  const promptModule = await import(promptModulePath)
  if (!("nextPrompt" in promptModule) || !isCallable(promptModule.nextPrompt)) {
    throw new Error(`Loop prompt module "${promptModulePath}" must export a callable nextPrompt.`)
  }

  return promptModule.nextPrompt
}

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

function toRuntimeLoopSession(
  session: ResolvedGoddardLoopConfigDocument["session"],
): AgentLoopParams["session"] {
  return session as AgentLoopParams["session"]
}

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

/** Builds runtime loop params from a resolved loop package plus overrides. */
export async function buildLoopParams(
  loop: ResolvedAgentLoop,
  overrides?: AgentLoopRuntimeOverrides,
): Promise<AgentLoopParams> {
  const { persistedLayer, retryableErrors } = splitLoopOverrides(overrides)
  const resolvedConfig = resolveLoopConfig(mergeLoopConfigLayers(loop.config, persistedLayer))

  return {
    nextPrompt: await importNextPrompt(loop.promptModulePath),
    session: toRuntimeLoopSession(resolvedConfig.session),
    rateLimits: resolvedConfig.rateLimits,
    retries: {
      ...resolvedConfig.retries,
      retryableErrors: retryableErrors ?? defaultRetryableErrors,
    },
  }
}

/** Resolves and runs a named loop package. */
export async function runNamedLoop(
  loopName: string,
  overrides?: AgentLoopRuntimeOverrides,
  handler?: AgentLoopHandler,
  options?: RunAgentOptions,
): Promise<AgentSession> {
  const cwd = overrides?.session?.cwd ?? process.cwd()
  return runAgentLoop(
    await buildLoopParams(await resolveLoop(loopName, cwd), overrides),
    handler,
    options,
  )
}

/** Runs an ad hoc loop from an explicit prompt module path. */
export async function runAdHocLoop(
  input: AdHocLoopInput,
  handler?: AgentLoopHandler,
  options?: RunAgentOptions,
): Promise<AgentSession> {
  const { promptModulePath: rawPromptModulePath, ...overrides } = input
  const promptModulePath = resolve(rawPromptModulePath)
  const { persistedLayer, retryableErrors } = splitLoopOverrides(overrides)
  const resolvedConfig = resolveLoopConfig(persistedLayer ?? {})

  const session = await runAgentLoop(
    {
      nextPrompt: await importNextPrompt(promptModulePath),
      session: toRuntimeLoopSession(resolvedConfig.session),
      rateLimits: resolvedConfig.rateLimits,
      retries: {
        ...resolvedConfig.retries,
        retryableErrors: retryableErrors ?? defaultRetryableErrors,
      },
    },
    handler,
    options,
  )

  exitHook(() => {
    void session.stop()
  })

  return session
}
