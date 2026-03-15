import type { RepoEvent } from "@goddard-ai/sdk"

export type FeedbackEvent = Extract<RepoEvent, { type: "comment" | "review" }>

export function isFeedbackEvent(event: RepoEvent): event is FeedbackEvent {
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
