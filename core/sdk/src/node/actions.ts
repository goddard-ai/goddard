import { mergeActionConfigLayers, type GoddardActionConfigDocument } from "@goddard-ai/config"
import type { NewSessionParams, SessionParams } from "@goddard-ai/schema/session-server"
import { existsSync } from "node:fs"
import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { runAgent } from "../daemon/session/client.ts"
import { readActionConfig, readMergedRootConfig } from "./config.ts"

/** Runtime overrides accepted when invoking a named action. */
export type AgentActionConfig = Omit<Partial<NewSessionParams>, "oneShot" | "initialPrompt">

/** A resolved named action prompt and merged persisted config. */
export type ResolvedAgentAction = {
  prompt: string
  config: GoddardActionConfigDocument
  path: string
}

const DEFAULT_AGENT = {
  id: "pi-coding-agent",
  name: "PI Coding Agent",
  version: "0.0.0",
  description: "Default PI coding agent distribution.",
  distribution: {
    npx: {
      package: "@mariozechner/pi-coding-agent",
    },
  },
} as const

function detectLegacyFrontmatter(content: string, path: string): void {
  if (content.startsWith("---\n") || content.startsWith("---\r\n")) {
    throw new Error(
      `Prompt file "${path}" uses YAML frontmatter. Persisted action config must move into JSON.`,
    )
  }
}

function ensureActionConfig(
  value: GoddardActionConfigDocument | undefined,
  path: string,
): GoddardActionConfigDocument {
  if (!value) {
    return {}
  }

  if (value == null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`Action config at ${path} must be an object.`)
  }

  return value
}

function toRuntimeActionConfig(config: GoddardActionConfigDocument): AgentActionConfig {
  return config as AgentActionConfig
}

async function loadMarkdownPrompt(path: string): Promise<string> {
  const content = await readFile(path, "utf-8")
  detectLegacyFrontmatter(content, path)
  return content
}

async function loadPromptOnlyAction(path: string): Promise<ResolvedAgentAction> {
  return {
    prompt: await loadMarkdownPrompt(path),
    config: {},
    path,
  }
}

async function loadPackagedAction(path: string): Promise<ResolvedAgentAction> {
  const promptPath = join(path, "prompt.md")
  const configPath = join(path, "config.json")

  if (!existsSync(promptPath)) {
    throw new Error(`Action directory "${path}" must include a prompt.md file.`)
  }

  if (!existsSync(configPath)) {
    throw new Error(`Action directory "${path}" must include a config.json file.`)
  }

  return {
    prompt: await loadMarkdownPrompt(promptPath),
    config: ensureActionConfig(await readActionConfig(configPath), configPath),
    path,
  }
}

async function resolveActionFromRoot(
  actionName: string,
  goddardRoot: string,
): Promise<ResolvedAgentAction | null> {
  const promptPath = join(goddardRoot, "actions", `${actionName}.md`)
  const folderPath = join(goddardRoot, "actions", actionName)
  const hasPromptFile = existsSync(promptPath)
  const hasFolder = existsSync(folderPath)

  if (hasPromptFile && hasFolder) {
    throw new Error(
      `Action "${actionName}" is ambiguous under "${goddardRoot}". Choose either a prompt file or a packaged directory.`,
    )
  }

  if (hasPromptFile) {
    return loadPromptOnlyAction(promptPath)
  }

  if (hasFolder) {
    return loadPackagedAction(folderPath)
  }

  return null
}

/** Builds daemon session params for a resolved action plus runtime overrides. */
export function buildActionSessionParams(
  action: ResolvedAgentAction,
  overrides?: AgentActionConfig,
): SessionParams & { oneShot: true } {
  return {
    agent: DEFAULT_AGENT,
    cwd: process.cwd(),
    mcpServers: [],
    ...toRuntimeActionConfig(action.config),
    ...overrides,
    oneShot: true as const,
    initialPrompt: action.prompt,
  }
}

/** Resolves a named action from local or global config roots. */
export async function resolveAction(
  actionName: string,
  cwd: string = process.cwd(),
): Promise<ResolvedAgentAction> {
  const { config, globalRoot, localRoot } = await readMergedRootConfig(cwd)
  const localAction = await resolveActionFromRoot(actionName, localRoot)
  const globalAction = localAction ? null : await resolveActionFromRoot(actionName, globalRoot)
  const action = localAction ?? globalAction

  if (!action) {
    throw new Error(
      `Action "${actionName}" not found in local or global configuration (.goddard/actions/<name>.md or .goddard/actions/<name>/prompt.md).`,
    )
  }

  return {
    ...action,
    config: mergeActionConfigLayers(
      ensureActionConfig(config.actions, "root config"),
      action.config,
    ),
  }
}

/** Runs a named action through the daemon-backed session client. */
export async function runAgentAction(actionName: string, options: AgentActionConfig) {
  const cwd = options.cwd ?? process.cwd()
  return runAgent(buildActionSessionParams(await resolveAction(actionName, cwd), options))
}
