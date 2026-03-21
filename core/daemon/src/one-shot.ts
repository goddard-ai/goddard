import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { readSocketPathFromDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { ManagedPrLocationStorage } from "@goddard-ai/storage/managed-pr-locations"
import { spawnSync } from "node:child_process"
import { prependAgentBinToPath } from "./config.js"
import type { FeedbackEvent } from "./feedback.js"
import { createDaemonLogger } from "./logging.js"
import * as prompts from "./prompts/index.js"

export type OneShotInput = {
  event: FeedbackEvent
  prompt: string
  daemonUrl: string
  agentBinDir: string
  env?: Record<string, string>
  resolveProjectDir?: (event: FeedbackEvent) => Promise<string | null> | string | null
}

function buildOneShotEnv(
  agentBinDir: string,
  inputEnv?: Record<string, string>,
): Record<string, string> {
  return prependAgentBinToPath(agentBinDir, inputEnv)
}

function renderPrompt(template: string, variables: Record<string, string>): string {
  const usedVariables = new Set<string>()
  const renderResult = template.replace(/\${(\w+)}/g, (_, key) => {
    const value = variables[key]
    if (typeof value !== "string") {
      throw new Error(`Prompt variable "${key}" is not a string`)
    }
    usedVariables.add(key)
    return value
  })

  if (usedVariables.size !== Object.keys(variables).length) {
    const unusedVariables = Object.keys(variables).filter((key) => !usedVariables.has(key))
    throw new Error(`Prompt variables were defined but never used: ${unusedVariables.join(", ")}`)
  }

  return renderResult
}

function buildBackgroundSystemPrompt(): string {
  return renderPrompt(prompts.BACKGROUND, {
    declare_initiative: prompts.CMD_DECLARE_INITIATIVE,
    report_blocker: prompts.CMD_REPORT_BLOCKER,
    global_rules: prompts.GLOBAL_RULES,
  })
}

export async function runOneShot(input: OneShotInput): Promise<number> {
  const logger = createDaemonLogger()
  const projectDir =
    (await input.resolveProjectDir?.(input.event)) ?? (await resolveProjectDir(input.event))
  if (!projectDir) {
    logger.log("one_shot.repository_lookup_failed", {
      repository: `${input.event.owner}/${input.event.repo}`,
      prNumber: input.event.prNumber,
    })
    return 1
  }

  const branchName = `pr-${input.event.prNumber}`
  const agentsDir = `${projectDir}/.goddard-agents`
  const worktreeDir = `${agentsDir}/${branchName}-${Date.now()}`

  spawnSync("mkdir", ["-p", agentsDir])

  try {
    let cpArgs = ["-R", `${projectDir}/`, worktreeDir]
    // Prefer copy-on-write cloning when the platform supports it, but fall back to a plain recursive copy.
    if (process.platform === "darwin") {
      cpArgs = ["-cR", `${projectDir}/`, worktreeDir]
    } else if (process.platform === "linux") {
      cpArgs = ["--reflink=auto", "-R", `${projectDir}/`, worktreeDir]
    }

    let cloneResult = spawnSync("cp", cpArgs, { encoding: "utf8" })
    let fallbackAttempted = false

    if (cloneResult.status !== 0 && process.platform === "darwin") {
      fallbackAttempted = true
      cpArgs = ["-R", `${projectDir}/`, worktreeDir]
      cloneResult = spawnSync("cp", cpArgs, { encoding: "utf8" })
    }

    if (cloneResult.status !== 0) {
      logger.log("one_shot.workspace_prepare_failed", {
        repository: `${input.event.owner}/${input.event.repo}`,
        prNumber: input.event.prNumber,
        worktreeDir,
        attemptedCommand: `cp ${cpArgs.join(" ")}`,
        fallbackAttempted,
        stderr: cloneResult.stderr?.trim() || undefined,
        errorMessage: cloneResult.error?.message,
      })
      return 1
    }
  } catch (error) {
    logger.log("one_shot.workspace_prepare_failed", {
      repository: `${input.event.owner}/${input.event.repo}`,
      prNumber: input.event.prNumber,
      worktreeDir,
      errorMessage: error instanceof Error ? error.message : String(error),
    })
    return 1
  }

  try {
    spawnSync("git", ["fetch", "origin", `pull/${input.event.prNumber}/head:${branchName}`], {
      cwd: worktreeDir,
      stdio: "ignore",
    })
    spawnSync("git", ["checkout", branchName], {
      cwd: worktreeDir,
      stdio: "ignore",
    })
  } catch {
    // Fall back to the copied checkout when the PR ref cannot be fetched into the scratch workspace.
    // Ignore git setup failures and let pi run against copied workspace state.
  }

  try {
    readSocketPathFromDaemonUrl(input.daemonUrl)
    const client = createDaemonIpcClient({ daemonUrl: input.daemonUrl })
    await client.send("sessionCreate", {
      agent: "pi",
      cwd: worktreeDir,
      mcpServers: [],
      initialPrompt: input.prompt,
      oneShot: true,
      systemPrompt: buildBackgroundSystemPrompt(),
      metadata: {
        repository: `${input.event.owner}/${input.event.repo}`,
        prNumber: input.event.prNumber,
      },
      gitUser: {
        username: input.event.author,
        email: `${input.event.author}@users.noreply.github.com`,
      },
      env: buildOneShotEnv(input.agentBinDir, input.env),
    })
    return 0
  } catch (error) {
    logger.log("one_shot.session_create_failed", {
      repository: `${input.event.owner}/${input.event.repo}`,
      prNumber: input.event.prNumber,
      daemonUrl: input.daemonUrl,
      worktreeDir,
      errorMessage: error instanceof Error ? error.message : String(error),
    })
    return 1
  }
}

async function resolveProjectDir(event: FeedbackEvent): Promise<string | null> {
  return (await ManagedPrLocationStorage.get(event.owner, event.repo, event.prNumber))?.cwd ?? null
}
