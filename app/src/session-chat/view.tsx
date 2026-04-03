import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useEffect } from "preact/hooks"
import { useSessionChat, useSessionIndex } from "~/app-state-context.tsx"
import { lookupSession } from "~/sessions/session-index.ts"
import { desktopSessionService } from "~/sessions/session-service.ts"
import { Composer } from "./composer.tsx"
import { Header } from "./header.tsx"
import { Transcript } from "./transcript.tsx"

export function View(props: { sessionId: string }) {
  const sessionChat = useSessionChat()
  const sessionIndex = useSessionIndex()
  const session = lookupSession(sessionIndex, props.sessionId)

  useEffect(() => {
    if (!session) {
      return
    }

    void sessionChat.loadThread(desktopSessionService, session)
  }, [session, sessionChat])

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
            shared session index.
          </p>
        </div>
      </div>
    )
  }

  const messages = sessionChat.messagesForSession(session.id)

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
          void sessionChat.promptSession(desktopSessionService, sessionIndex, session, text)
        }}
      />
    </div>
  )
}

export default View
