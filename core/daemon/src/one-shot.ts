import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { readSocketPathFromDaemonUrl } from "@goddard-ai/schema/daemon-url"
import type { FeedbackEvent } from "./feedback.ts"
import { createDaemonLogger } from "./logging.ts"

/** Input required to route one feedback event back into its original PR creator session. */
export type OneShotInput = {
  event: FeedbackEvent
  prompt: string
  daemonUrl: string
}

export async function runOneShot(input: OneShotInput): Promise<string | null> {
  const logger = createDaemonLogger()

  try {
    readSocketPathFromDaemonUrl(input.daemonUrl)
    const client = createDaemonIpcClient({ daemonUrl: input.daemonUrl })
    const response = await client.send("prFeedbackResume", {
      owner: input.event.owner,
      repo: input.event.repo,
      prNumber: input.event.prNumber,
      prompt: input.prompt,
    })
    return response.sessionId
  } catch (error) {
    logger.log("one_shot.resume_failed", {
      repository: `${input.event.owner}/${input.event.repo}`,
      prNumber: input.event.prNumber,
      daemonUrl: input.daemonUrl,
      errorMessage: error instanceof Error ? error.message : String(error),
    })
    return null
  }
}
