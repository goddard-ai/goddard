import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"
import { useSigma } from "preact-sigma"
import { useState } from "preact/hooks"
import { isPromise } from "radashi"

import { SessionChat } from "./chat.ts"
import { Composer } from "./composer.tsrx"
import { Transcript } from "./transcript.tsrx"
import styles from "./view.style.ts"
import { queryClient, useQueries } from "~/lib/query.ts"
import { useNearestProjectPath, useReportTabProject } from "~/projects/use-tab-project.ts"
import { goddardSdk } from "~/sdk.ts"
import { submitSessionPrompt } from "~/sessions/mutations.ts"

export component SessionChatView(props: {
  projectPath: string | null;
  sessionId: DaemonSession["id"];
}) {
  const [, setRetryVersion] = useState(0)

  try {
    const [sessionHistory, loadedSession] = useQueries([
      [goddardSdk.session.history, [{ id: props.sessionId }]],
      [goddardSdk.session.get, [{ id: props.sessionId }]],
    ])

    <LoadedSessionChatView {...props} history={sessionHistory} session={loadedSession.session} />
  } catch (error) {
    if (isPromise(error)) {
      <SessionChatLoadingState />
    } else {
      <main class={styles.root}>
        <SessionChatStateMessage
          actionLabel="Try again"
          description="The daemon-backed session or history request failed."
          onAction={() => {
            retrySessionChatLoad(props.sessionId)
            setRetryVersion((version) => version + 1)
          }}
          title="Couldn't load session"
        />
      </main>
    }
  }
}

component SessionChatLoadingState() {
  <main class={styles.root}>
    <SessionChatStateMessage
      description="Loading the session transcript and current session details."
      title="Loading session"
    />
  </main>
}

component SessionChatStateMessage(props: {
  actionLabel?: string;
  description: string;
  onAction?: () => void;
  title: string;
}) {
  <div class={styles.stateMessage}>
    <h2 class={styles.stateTitle}>{props.title}</h2>
    <p class={styles.stateDescription}>{props.description}</p>
    if (props.actionLabel && props.onAction) {
      <button class={styles.stateAction} type="button" onClick={props.onAction}>
        {props.actionLabel}
      </button>
    }
  </div>
}

/** Retries both reads required for the current query-driven session chat view. */
function retrySessionChatLoad(sessionId: DaemonSession["id"]) {
  queryClient.invalidate(goddardSdk.session.get, [{ id: sessionId }])
  queryClient.invalidate(goddardSdk.session.history, [{ id: sessionId }])
}

component LoadedSessionChatView(props: {
  history: GetSessionHistoryResponse;
  projectPath: string | null;
  session: DaemonSession;
}) {
  const chat = useSigma(() => new SessionChat({ session: props.session, turns: props.history.turns }), [
    props.session,
    props.history.turns,
  ])
  const hasEmptyTranscript = props.history.turns.length === 0 && !props.session.lastAgentMessage
  const resolvedProjectPath = useNearestProjectPath(props.session.cwd)

  useReportTabProject(resolvedProjectPath)

  <main class={styles.root}>
    if (hasEmptyTranscript) {
      <div class={styles.transcript}>
        <SessionChatStateMessage
          description="Send a prompt to start this session."
          title="No messages yet"
        />
      </div>
    } else {
      <Transcript
        class={styles.transcript}
        initialScrollPosition="bottom"
        messages={chat.messages}
        scrollCacheKey={`detail:session:${props.session.id}:transcript`}
      />
    }
    <footer class={styles.composerDock}>
      <Composer
        loadSuggestions={async (input) => {
          const response = await goddardSdk.session.composerSuggestions(
            {
              id: props.session.id,
              trigger: input.trigger,
              query: input.query,
            },
          )

          return response.suggestions
        }}
        onSubmit={async (prompt) => {
          try {
            await submitSessionPrompt(
              {
                id: props.session.id,
                acpId: props.session.acpSessionId,
                prompt,
              },
            )
          } catch (error) {
            console.error("Failed to submit session prompt.", error)
          }
        }}
      />
    </footer>
  </main>
}

export default SessionChatView
