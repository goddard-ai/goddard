import type { BackendEventHandler, DaemonLogService, EventBus } from "@goddard-ai/daemon-plugin"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"
import type { CreateSessionRequest } from "@goddard-ai/session/schema"

import type { pullRequestEvents } from "../events.ts"
import type { DaemonPullRequest } from "../schema.ts"

export type FeedbackEvent = Extract<RepoEvent, { type: "comment" | "review" }>

type FeedbackBackend = {
  readonly pullRequests: {
    readonly managed: (input: {
      readonly owner: string
      readonly repo: string
      readonly prNumber: number
    }) => Promise<{ readonly managed: boolean }>
  }
}

type FeedbackStore = {
  readonly pullRequests: {
    readonly first: (query: {
      readonly where: {
        readonly host: "github"
        readonly owner: string
        readonly repo: string
        readonly prNumber: number
      }
    }) => Pick<DaemonPullRequest, "cwd"> | null | undefined
  }
}

type FeedbackSessionService = {
  readonly newSession: (input: { readonly request: CreateSessionRequest }) => Promise<unknown>
}

type FeedbackEventBus = Pick<EventBus<typeof pullRequestEvents>, "emit">

export function isFeedbackEvent(event: unknown): event is FeedbackEvent {
  if (!event || typeof event !== "object" || !("type" in event)) {
    return false
  }

  return event.type === "comment" || event.type === "review"
}

export function buildPrompt(event: FeedbackEvent): string {
  const feedback =
    event.type === "comment"
      ? `Comment from @${event.author}:\n${event.body}`
      : `Review from @${event.author} (${event.state}):\n${event.body}`

  return [
    `You are responding to PR feedback for ${event.owner}/${event.repo}#${event.prNumber}.`,
    feedback,
    "Assess the feedback, apply any necessary repository changes, and finish by posting a reply on that PR thread explaining what you changed or why no change was needed.",
    "Write your reply summary to a text file and post it with the session CLI.",
    "Use: `goddard reply-pr --message-file reply.txt`",
    "Do not switch to another PR; stay scoped to this event's PR.",
  ].join("\n\n")
}

export function createPullRequestFeedbackHandler(input: {
  readonly backend: FeedbackBackend
  readonly db: FeedbackStore
  readonly events: FeedbackEventBus
  readonly log: DaemonLogService
  readonly session: FeedbackSessionService
}): BackendEventHandler<FeedbackEvent> {
  const logger = input.log.createLogger()
  const runningPrs = new Set<string>()

  return {
    name: "pull-request.feedback",
    canHandle: isFeedbackEvent,
    async handle(event) {
      const feedbackContext = {
        repository: `${event.owner}/${event.repo}`,
        prNumber: event.prNumber,
        feedbackType: event.type,
      }
      const feedbackEventPayload = {
        ...feedbackContext,
        owner: event.owner,
        repo: event.repo,
      }
      const requestKey = `${event.owner}/${event.repo}#${event.prNumber}`

      if (runningPrs.has(requestKey)) {
        logger.log("repo.feedback_coalesced", {
          feedbackEvent: feedbackContext,
        })
        return
      }

      runningPrs.add(requestKey)

      try {
        const { managed } = await input.backend.pullRequests.managed({
          owner: event.owner,
          repo: event.repo,
          prNumber: event.prNumber,
        })
        if (!managed) {
          logger.log("repo.feedback_ignored", {
            feedbackEvent: feedbackContext,
            reason: "unmanaged_pr",
          })
          await input.events.emit("pull_request.feedback.ignored", {
            ...feedbackEventPayload,
            reason: "unmanaged_pr",
          })
          return
        }

        const projectDir = resolveProjectDir(input.db, event)
        if (!projectDir) {
          logger.log("pr_feedback.repository_lookup_failed", {
            repository: feedbackContext.repository,
            prNumber: event.prNumber,
          })
          await input.events.emit("pull_request.feedback.finished", {
            ...feedbackEventPayload,
            exitCode: 1,
          })
          return
        }

        const prompt = buildPrompt(event)
        logger.log("pr_feedback.launch", {
          feedbackEvent: feedbackContext,
          prompt: input.log.createPayloadPreview(prompt),
        })
        await input.session.newSession({
          request: {
            cwd: projectDir,
            worktree: { enabled: true },
            mcpServers: [],
            initialPrompt: prompt,
            oneShot: true,
            repository: feedbackContext.repository,
            prNumber: event.prNumber,
          },
        })
        logger.log("pr_feedback.finish", {
          feedbackEvent: feedbackContext,
          exitCode: 0,
        })
        await input.events.emit("pull_request.feedback.finished", {
          ...feedbackEventPayload,
          exitCode: 0,
        })
      } catch (error) {
        logger.log("pr_feedback.failed", {
          feedbackEvent: feedbackContext,
          errorMessage: error instanceof Error ? error.message : String(error),
        })
      } finally {
        runningPrs.delete(requestKey)
      }
    },
  }
}

function resolveProjectDir(store: FeedbackStore, event: FeedbackEvent): string | null {
  return (
    store.pullRequests.first({
      where: {
        host: "github",
        owner: event.owner,
        repo: event.repo,
        prNumber: event.prNumber,
      },
    })?.cwd ?? null
  )
}
