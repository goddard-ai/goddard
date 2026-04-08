import type { DaemonSession } from "@goddard-ai/sdk"
import { useMutation, useQuery, useQueryClient } from "@tanstack/preact-query"
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Composer } from "./composer.tsx"
import { buildTranscriptMessages } from "./chat.ts"
import { Header } from "./header.tsx"
import { Transcript } from "./transcript.tsx"
import {
  getSessionHistoryQueryOptions,
  getSessionQueryOptions,
  sessionQueryKeys,
} from "~/sessions/queries.ts"
import { goddardSdk } from "~/sdk.ts"

export function SessionChatView(props: { sessionId: string }) {
  const queryClient = useQueryClient()
  const sessionId = props.sessionId as DaemonSession["id"]
  const sessionQuery = useQuery(getSessionQueryOptions(sessionId))
  const session = sessionQuery.data ?? null
  const sessionHistoryQuery = useQuery({
    ...getSessionHistoryQueryOptions(sessionId),
    enabled: session !== null,
  })
  const promptSessionMutation = useMutation({
    mutationFn: async (prompt: string) => {
      if (!session) {
        return null
      }

      const trimmedPrompt = prompt.trim()

      if (trimmedPrompt.length === 0) {
        return null
      }

      await goddardSdk.session.prompt({
        id: session.id,
        acpId: session.acpSessionId,
        prompt: trimmedPrompt,
      })

      const response = await goddardSdk.session.get({ id: session.id })
      return response.session
    },
    onSuccess: async (nextSession) => {
      if (!nextSession) {
        return
      }

      queryClient.setQueryData(sessionQueryKeys.detail(nextSession.id), nextSession)
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: sessionQueryKeys.lists() }),
        queryClient.invalidateQueries({ queryKey: sessionQueryKeys.history(nextSession.id) }),
      ])
    },
  })

  if (sessionQuery.isPending) {
    return (
      <div
        class={css({
          display: "grid",
          placeItems: "center",
          height: "100%",
          padding: "28px",
          background:
            `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 12%, transparent), transparent 28%), ` +
            `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
        })}
      >
        <div
          class={css({
            color: "muted",
            fontSize: "0.95rem",
          })}
        >
          Loading session...
        </div>
      </div>
    )
  }

  if (!session) {
    return (
      <div
        class={css({
          display: "grid",
          placeItems: "center",
          height: "100%",
          padding: "28px",
          background:
            `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 12%, transparent), transparent 28%), ` +
            `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
        })}
      >
        <div
          class={css({
            maxWidth: "32rem",
            padding: "28px",
            borderRadius: "26px",
            border: "1px solid",
            borderColor: "border",
            background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
          })}
        >
          <h1
            class={css({
              marginBottom: "10px",
              color: "text",
              fontSize: "1.2rem",
              fontWeight: "740",
            })}
          >
            Session unavailable
          </h1>
          <p
            class={css({
              color: "muted",
              lineHeight: "1.7",
            })}
          >
            The chat tab still exists, but the backing session record is no longer present in the
            daemon session store.
          </p>
        </div>
      </div>
    )
  }

  const messages = buildTranscriptMessages(session, sessionHistoryQuery.data?.history ?? [])

  return (
    <div
      class={css({
        display: "grid",
        gridTemplateRows: "auto minmax(0, 1fr) auto",
        height: "100%",
        background:
          `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 12%, transparent), transparent 28%), ` +
          `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
      })}
    >
      <Header messageCount={messages.length} session={session} />
      <Transcript
        initialScrollPosition="bottom"
        messages={messages}
        scrollCacheKey={`detail:session:${session.id}:transcript`}
      />
      <Composer
        onSubmit={(text) => {
          void promptSessionMutation.mutateAsync(text)
        }}
      />
    </div>
  )
}

export default SessionChatView
