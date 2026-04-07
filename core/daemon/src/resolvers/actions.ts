import { mergeActionConfigLayers, resolveDefaultAgent } from "@goddard-ai/config"
import { ActionConfig, type InlineSessionParams } from "@goddard-ai/schema/config"
import type { CreateDaemonSessionRequest } from "@goddard-ai/schema/daemon"
import { existsSync } from "node:fs"
import { readFile } from "node:fs/promises"
import { join, resolve } from "node:path"
import type { RootConfigProvider } from "./config.ts"
import { readActionConfig, readCurrentRootConfig } from "./config.ts"

/** A resolved named action prompt and merged persisted config. */
export type ResolvedAction = {
  prompt: string
  config: ActionConfig
  path: string
}

/** Rejects legacy prompt frontmatter that now belongs in JSON config. */
function detectLegacyFrontmatter(content: string, path: string): void {
  if (content.startsWith("---\n") || content.startsWith("---\r\n")) {
    throw new Error(
      `Prompt file "${path}" uses YAML frontmatter. Persisted action config must move into JSON.`,
    )
  }
}

/** Validates that one parsed action config is an object when present. */
function ensureActionConfig(value: ActionConfig | undefined, path: string): ActionConfig {
  if (!value) {
    return {}
  }

  if (value == null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`Action config at ${path} must be an object.`)
  }

  return value
}

/** Loads one markdown prompt file and rejects legacy frontmatter. */
async function loadMarkdownPrompt(path: string): Promise<string> {
  const content = await readFile(path, "utf-8")
  detectLegacyFrontmatter(content, path)
  return content
}

/** Loads one prompt-only action package. */
async function loadPromptOnlyAction(path: string): Promise<ResolvedAction> {
  return {
    prompt: await loadMarkdownPrompt(path),
    config: {},
    path,
  }
}

/** Loads one packaged action directory and validates its required files. */
async function loadPackagedAction(path: string): Promise<ResolvedAction> {
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

/** Resolves one action name from a specific `.goddard` root. */
async function resolveActionFromRoot(
  actionName: string,
  goddardRoot: string,
): Promise<ResolvedAction | null> {
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

/** Resolves one named action from local or global config roots. */
export async function resolveNamedAction(
  actionName: string,
  cwd: string,
  rootConfigProvider?: RootConfigProvider,
): Promise<ResolvedAction> {
  const resolvedCwd = resolve(cwd)
  const { config, globalRoot, localRoot } = await readCurrentRootConfig(
    resolvedCwd,
    rootConfigProvider,
  )
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

/** Builds daemon session params for one resolved action plus runtime overrides. */
export function buildNamedActionSessionParams(
  action: ResolvedAction,
  cwd: string,
  params: InlineSessionParams = {},
): CreateDaemonSessionRequest & { oneShot: true } {
  const sessionConfig = action.config.session ?? {}

  return {
    agent: params.agent ?? sessionConfig.agent ?? "pi-acp",
    cwd: resolve(params.cwd ?? cwd),
    mcpServers: params.mcpServers ?? sessionConfig.mcpServers ?? [],
    env: params.env ?? sessionConfig.env,
    systemPrompt: params.systemPrompt ?? "",
    repository: params.repository,
    prNumber: params.prNumber,
    metadata: params.metadata,
    oneShot: true as const,
    initialPrompt: action.prompt,
  }
}
