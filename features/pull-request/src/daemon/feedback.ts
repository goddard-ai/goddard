import type { RouzerClient } from "@goddard-ai/backend-plugin"
import type {
  BackendEventHandler,
  DaemonLogService,
  EventBus,
  InferProvides,
} from "@goddard-ai/daemon-plugin"
import {
  REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
  REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED,
  type RemoteRepoPullRequestCommentCreatedEvent,
  type RemoteRepoPullRequestReviewSubmittedEvent,
} from "@goddard-ai/remote-repo/backend"
import {
  RepoPullRequestCommentCreatedEvent,
  RepoPullRequestReviewSubmittedEvent,
} from "@goddard-ai/remote-repo/schema"
import type { sessionPlugin } from "@goddard-ai/session/daemon"
import { z } from "zod"

import type { pullRequestBackendRoutes } from "../backend.ts"
import type { PullRequestDb } from "../daemon.ts"
import type { pullRequestEvents } from "../events.ts"

export const FeedbackEvent = z.discriminatedUnion("type", [
  RepoPullRequestCommentCreatedEvent,
  RepoPullRequestReviewSubmittedEvent,
])

export type FeedbackEvent = z.infer<typeof FeedbackEvent>

export type FeedbackBackendEvent =
  | RemoteRepoPullRequestCommentCreatedEvent
  | RemoteRepoPullRequestReviewSubmittedEvent

const FeedbackBackendEvent = z.discriminatedUnion("name", [
  z.object({
    name: z.literal(REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED),
    payload: RepoPullRequestCommentCreatedEvent,
  }),
  z.object({
    name: z.literal(REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED),
    payload: RepoPullRequestReviewSubmittedEvent,
  }),
])

export function isFeedbackEvent(event: unknown): event is FeedbackEvent {
  return FeedbackEvent.safeParse(event).success
}

export function isFeedbackBackendEvent(event: unknown): event is FeedbackBackendEvent {
  return FeedbackBackendEvent.safeParse(event).success
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
  readonly backend: {
    readonly pullRequests: Pick<
      RouzerClient<typeof pullRequestBackendRoutes>["pullRequests"],
      "managed"
    >
  }
  readonly db: {
    readonly pullRequests: Pick<PullRequestDb["pullRequests"], "first">
  }
  readonly events: Pick<EventBus<typeof pullRequestEvents>, "emit">
  readonly log: DaemonLogService
  readonly session: {
    readonly newSession: (
      input: Parameters<InferProvides<typeof sessionPlugin>["session"]["newSession"]>[0],
    ) => Promise<unknown>
  }
}): BackendEventHandler<FeedbackBackendEvent> {
  const runningPrs = new Set<string>()

  return {
    name: "pull-request.feedback",
    canHandle: isFeedbackBackendEvent,
    async handle(envelope) {
      const event = envelope.payload
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
        await input.events.emit("pull_request.feedback.coalesced", feedbackEventPayload)
        return
      }

      runningPrs.add(requestKey)

      try {
        const { managed } = await input.backend.pullRequests.managed({
          provider: event.provider,
          owner: event.owner,
          repo: event.repo,
          prNumber: event.prNumber,
        })
        if (!managed) {
          await input.events.emit("pull_request.feedback.ignored", {
            ...feedbackEventPayload,
            reason: "unmanaged_pr",
          })
          return
        }

        const projectDir = resolveProjectDir(input.db, event)
        if (!projectDir) {
          await input.events.emit("pull_request.feedback.failed", {
            ...feedbackEventPayload,
            phase: "repository_lookup",
            errorMessage: "Managed pull request repository is unavailable",
          })
          await input.events.emit("pull_request.feedback.finished", {
            ...feedbackEventPayload,
            exitCode: 1,
          })
          return
        }

        const prompt = buildPrompt(event)
        await input.events.emit("pull_request.feedback.launched", feedbackEventPayload)
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
        await input.events.emit("pull_request.feedback.finished", {
          ...feedbackEventPayload,
          exitCode: 0,
        })
      } catch (error) {
        await input.events.emit("pull_request.feedback.failed", {
          ...feedbackEventPayload,
          phase: "session_create",
          errorMessage: error instanceof Error ? error.message : String(error),
        })
      } finally {
        runningPrs.delete(requestKey)
      }
    },
  }
}

function resolveProjectDir(
  store: { readonly pullRequests: Pick<PullRequestDb["pullRequests"], "first"> },
  event: FeedbackEvent,
) {
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
