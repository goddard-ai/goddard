import type { NewSessionParams, SessionParams } from "@goddard-ai/schema/session-server"
import { runAgent } from "@goddard-ai/session"
import { getGoddardGlobalDir } from "@goddard-ai/storage"
import { existsSync } from "node:fs"
import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { parse as parseYaml } from "yaml"

export type ResolvedAgentAction = {
  prompt: string
  config: Partial<NewSessionParams>
  path: string
}

export type RunAgentActionParams = Partial<NewSessionParams> & {
  initialPrompt: NonNullable<NewSessionParams["initialPrompt"]>
}

const DEFAULT_AGENT = { type: "npx", package: "@mariozechner/pi-coding-agent" } as const

function hasErrorCode(error: unknown, code: string): error is { code: string } {
  return typeof error === "object" && error !== null && "code" in error && error.code === code
}

function ensureConfigObject(value: unknown, path: string): Partial<NewSessionParams> {
  if (value == null) {
    return {}
  }

  if (typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`Action config at ${path} must be an object.`)
  }

  return value as Partial<NewSessionParams>
}

function parseMarkdownFrontmatter(content: string, path: string): ResolvedAgentAction {
  const match = content.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/)
  if (!match) {
    return { prompt: content, config: {}, path }
  }

  const [, rawFrontmatter, prompt] = match
  const parsed = parseYaml(rawFrontmatter)

  return {
    prompt,
    config: ensureConfigObject(parsed, path),
    path,
  }
}

async function loadMarkdownAction(path: string): Promise<ResolvedAgentAction> {
  const content = await readFile(path, "utf-8")
  return parseMarkdownFrontmatter(content, path)
}

async function loadFolderAction(path: string): Promise<ResolvedAgentAction> {
  const promptPath = join(path, "prompt.md")
  let promptAction: ResolvedAgentAction

  try {
    promptAction = await loadMarkdownAction(promptPath)
  } catch (error) {
    // Require a prompt.md file if the action directory exists
    if (hasErrorCode(error, "ENOENT") && existsSync(path)) {
      throw new Error(`Action directory "${path}" must include a prompt.md file.`)
    }
    throw error
  }

  const configPath = join(path, "config.json")
  let config = promptAction.config

  try {
    const rawConfig = await readFile(configPath, "utf-8")
    config = {
      ...config,
      ...ensureConfigObject(JSON.parse(rawConfig), configPath),
    }
  } catch (error) {
    if (!hasErrorCode(error, "ENOENT")) {
      throw error
    }
  }

  return {
    prompt: promptAction.prompt,
    config,
    path,
  }
}

async function resolveActionFromRoot(
  actionName: string,
  goddardRoot: string,
): Promise<ResolvedAgentAction | null> {
  const mdPath = join(goddardRoot, "actions", `${actionName}.md`)
  try {
    return await loadMarkdownAction(mdPath)
  } catch (error) {
    if (!hasErrorCode(error, "ENOENT")) {
      throw error
    }
  }

  const folderPath = join(goddardRoot, "actions", actionName)
  try {
    return await loadFolderAction(folderPath)
  } catch (error) {
    if (!hasErrorCode(error, "ENOENT")) {
      throw error
    }
  }

  return null
}

export function buildActionSessionParams(
  action: ResolvedAgentAction,
  options: RunAgentActionParams,
): SessionParams {
  const mergedConfig = {
    agent: DEFAULT_AGENT,
    cwd: process.cwd(),
    mcpServers: [],
    oneShot: true,
    ...options,
    ...action.config,
  }

  return {
    ...mergedConfig,
    oneShot: mergedConfig.oneShot === true ? true : undefined,
    systemPrompt: [mergedConfig.systemPrompt, action.prompt].filter(Boolean).join("\n\n"),
  }
}

export async function resolveAction(
  actionName: string,
  cwd: string = process.cwd(),
): Promise<ResolvedAgentAction> {
  const localAction = await resolveActionFromRoot(actionName, join(cwd, ".goddard"))
  if (localAction) {
    return localAction
  }

  const globalAction = await resolveActionFromRoot(actionName, getGoddardGlobalDir())
  if (globalAction) {
    return globalAction
  }

  throw new Error(
    `Action "${actionName}" not found in local or global configuration (.goddard/actions/*.md or .goddard/actions/<action>/prompt.md).`,
  )
}

export async function runAgentAction(actionName: string, options: RunAgentActionParams) {
  const cwd = options.cwd ?? process.cwd()
  const action = await resolveAction(actionName, cwd)
  return runAgent(buildActionSessionParams(action, options))
}
