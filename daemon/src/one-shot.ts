import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { spawnSync } from "node:child_process"
import { readSocketPathFromDaemonUrl } from "@goddard-ai/schema/daemon-url"
import * as prompts from "./prompts/index.ts"
import { prependAgentBinToPath } from "./config.ts"
import type { FeedbackEvent } from "./feedback.ts"
import { createDaemonLogger } from "./logging.ts"

export type OneShotInput = {
  event: FeedbackEvent
  prompt: string
  projectDir: string
  daemonUrl: string
  agentBinDir: string
  env?: Record<string, string>
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
  const branchName = `pr-${input.event.prNumber}`
  const agentsDir = `${input.projectDir}/.goddard-agents`
  const worktreeDir = `${agentsDir}/${branchName}-${Date.now()}`

  spawnSync("mkdir", ["-p", agentsDir])

  try {
    let cpArgs = ["-R", `${input.projectDir}/`, worktreeDir]
    if (process.platform === "darwin") {
      cpArgs = ["-cR", `${input.projectDir}/`, worktreeDir]
    } else if (process.platform === "linux") {
      cpArgs = ["--reflink=auto", "-R", `${input.projectDir}/`, worktreeDir]
    }

    let cloneResult = spawnSync("cp", cpArgs, { encoding: "utf8" })
    let fallbackAttempted = false

    if (cloneResult.status !== 0 && process.platform === "darwin") {
      fallbackAttempted = true
      cpArgs = ["-R", `${input.projectDir}/`, worktreeDir]
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
