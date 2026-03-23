import { mergeActionConfigLayers, resolveDefaultAgent } from "@goddard-ai/config"
import { ActionConfig, InlineSessionParams } from "@goddard-ai/schema/config"
import type { SessionParams } from "@goddard-ai/schema/session-server"
import { existsSync } from "node:fs"
import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { runAgent } from "../daemon/session/client.ts"
import { readActionConfig, readMergedRootConfig } from "./config.ts"

/** A resolved named action prompt and merged persisted config. */
export type ResolvedAgentAction = {
  prompt: string
  config: ActionConfig
  path: string
}

function detectLegacyFrontmatter(content: string, path: string): void {
  if (content.startsWith("---\n") || content.startsWith("---\r\n")) {
    throw new Error(
      `Prompt file "${path}" uses YAML frontmatter. Persisted action config must move into JSON.`,
    )
  }
}

function ensureActionConfig(value: ActionConfig | undefined, path: string): ActionConfig {
  if (!value) {
    return {}
  }

  if (value == null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`Action config at ${path} must be an object.`)
  }

  return value
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
  params?: InlineSessionParams,
): SessionParams & { oneShot: true } {
  const sessionConfig = action.config.session ?? {}

  return {
    agent: sessionConfig.agent ?? "pi-acp",
    cwd: process.cwd(),
    mcpServers: [],
    ...sessionConfig,
    ...params,
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

  const mergedConfig = mergeActionConfigLayers(
    config.session ? { session: config.session } : undefined,
    ensureActionConfig(config.actions, "root config"),
    action.config,
  )
  const sessionConfig = mergedConfig.session ?? {}
  sessionConfig.agent = sessionConfig.agent ?? (await resolveDefaultAgent(config, "actions"))
  mergedConfig.session = sessionConfig

  return {
    ...action,
    config: mergedConfig,
  }
}

/** Runs a named action through the daemon-backed session client. */
export async function runAgentAction(actionName: string, params: InlineSessionParams = {}) {
  const action = await resolveAction(actionName, params.cwd)
  return runAgent(buildActionSessionParams(action, params))
}
