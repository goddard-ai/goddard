import { resolveDefaultAgent } from "@goddard-ai/config"
/** Launches daemon-managed sessions for individual PR feedback events. */
import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { readDaemonTcpAddressFromDaemonUrl } from "@goddard-ai/schema/daemon-url"

import type { ConfigManager } from "./config-manager.ts"
import { prependAgentBinToPath } from "./config.ts"
import type { FeedbackEvent } from "./feedback.ts"
import { createLogger } from "./logging.ts"
import { db } from "./persistence/store.ts"
import * as prompts from "./prompts/index.ts"

/** Launch input for a daemon-managed PR feedback flow. */
export type PrFeedbackFlowInput = {
  event: FeedbackEvent
  prompt: string
  daemonUrl: string
  agentBinDir: string
  env?: Record<string, string>
  configManager?: Pick<ConfigManager, "getRootConfig">
  resolveProjectDir?: (event: FeedbackEvent) => Promise<string | null> | string | null
}

function buildPrFeedbackEnv(
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

/** Creates the daemon session that handles one managed PR feedback event within the flow. */
export async function runPrFeedbackFlow(input: PrFeedbackFlowInput): Promise<number> {
  const logger = createLogger()
  const projectDir =
    (await input.resolveProjectDir?.(input.event)) ?? (await resolveProjectDir(input.event))
  if (!projectDir) {
    logger.log("pr_feedback.repository_lookup_failed", {
      repository: `${input.event.owner}/${input.event.repo}`,
      prNumber: input.event.prNumber,
    })
    return 1
  }

  try {
    readDaemonTcpAddressFromDaemonUrl(input.daemonUrl)
    const rootConfig = input.configManager
      ? (await input.configManager.getRootConfig(projectDir)).config
      : undefined
    const client = createDaemonIpcClient({ daemonUrl: input.daemonUrl })
    await client.send("session.create", {
      agent: await resolveDefaultAgent(rootConfig),
      cwd: projectDir,
      worktree: { enabled: true },
      mcpServers: [],
      initialPrompt: input.prompt,
      oneShot: true,
      systemPrompt: buildBackgroundSystemPrompt(),
      repository: `${input.event.owner}/${input.event.repo}`,
      prNumber: input.event.prNumber,
      env: buildPrFeedbackEnv(input.agentBinDir, input.env),
    })
    return 0
  } catch (error) {
    logger.log("pr_feedback.session_create_failed", {
      repository: `${input.event.owner}/${input.event.repo}`,
      prNumber: input.event.prNumber,
      daemonUrl: input.daemonUrl,
      cwd: projectDir,
      errorMessage: error instanceof Error ? error.message : String(error),
    })
    return 1
  }
}

async function resolveProjectDir(event: FeedbackEvent): Promise<string | null> {
  return (
    db.pullRequests.first({
      where: {
        host: "github",
        owner: event.owner,
        repo: event.repo,
        prNumber: event.prNumber,
      },
    })?.cwd ?? null
  )
}
