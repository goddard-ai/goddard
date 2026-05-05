import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"
import { useEffect, useState } from "preact/hooks"
import { useSigma } from "preact-sigma"
import { isPromise } from "radashi"

import { Composer } from "./composer.tsrx"
import { SessionChatModel } from "./model.ts"
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
  const [submitError, setSubmitError] = useState<unknown>(null)
  const chat = useSigma(
    () => new SessionChatModel({
      history: props.history,
      session: props.session,
    }),
    { deps: [props.session.id] },
  )
  const resolvedProjectPath = useNearestProjectPath(chat.session.cwd)

  useEffect(() => {
    chat.syncLoadedData(
      {
        history: props.history,
        session: props.session,
      },
    )
  }, [chat, props.history, props.session])

  useReportTabProject(resolvedProjectPath)

  <main class={styles.root}>
    if (chat.hasEmptyTranscript) {
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
        messages={chat.transcriptMessages}
        scrollCacheKey={`detail:session:${chat.session.id}:transcript`}
      />
    }
    <footer class={styles.composerDock}>
      if (submitError) {
        <div class={styles.composerError} role="alert">
          <strong class={styles.composerErrorTitle}>{"Prompt wasn't sent"}</strong>
          <span class={styles.composerErrorDescription}>
            {"Your draft is still here. Try again when the session is available."}
          </span>
        </div>
      }
      <Composer
        loadSuggestions={async (input) => {
          const response = await goddardSdk.session.composerSuggestions(
            {
              id: chat.session.id,
              trigger: input.trigger,
              query: input.query,
            },
          )

          return response.suggestions
        }}
        onPromptChange={() => {
          if (submitError) {
            setSubmitError(null)
          }
        }}
        onSubmit={async (prompt) => {
          setSubmitError(null)

          try {
            await submitSessionPrompt(
              {
                id: chat.session.id,
                acpId: chat.session.acpSessionId,
                prompt,
              },
            )
          } catch (error) {
            console.error("Failed to submit session prompt.", error)
            setSubmitError(error)
            throw error
          }
        }}
      />
    </footer>
  </main>
}

export default SessionChatView
