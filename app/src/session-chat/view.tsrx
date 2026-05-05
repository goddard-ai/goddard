import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"
import {
  AlertCircle,
  CheckCircle2,
  CircleDot,
  LoaderCircle,
  PauseCircle,
  XCircle,
} from "lucide-react"
import { useEffect, useState } from "preact/hooks"
import { useSigma } from "preact-sigma"
import { isPromise } from "radashi"

import { Composer } from "./composer.tsrx"
import { SessionChat } from "./model.ts"
import type { SessionChatStatus } from "./state.ts"
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

function getSessionChatStatusPresentation(status: SessionChatStatus) {
  switch (status) {
    case "running":
      return {
        description: "Agent is working",
        icon: LoaderCircle,
        label: "Running",
        tone: "running",
      }
    case "blocked":
      return {
        description: "Waiting for attention",
        icon: PauseCircle,
        label: "Blocked",
        tone: "blocked",
      }
    case "completed":
      return {
        description: "Session finished",
        icon: CheckCircle2,
        label: "Completed",
        tone: "completed",
      }
    case "failed":
      return {
        description: "Session failed",
        icon: AlertCircle,
        label: "Failed",
        tone: "failed",
      }
    case "cancelled":
      return {
        description: "Session cancelled",
        icon: XCircle,
        label: "Cancelled",
        tone: "cancelled",
      }
    default:
      return {
        description: "Ready for a prompt",
        icon: CircleDot,
        label: "Ready",
        tone: "ready",
      }
  }
}

component LoadedSessionChatView(props: {
  history: GetSessionHistoryResponse;
  projectPath: string | null;
  session: DaemonSession;
}) {
  const [submitError, setSubmitError] = useState<unknown>(null)
  const chat = useSigma(
    () => new SessionChat({
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

  const status = getSessionChatStatusPresentation(chat.summary.status)
  const StatusIcon = status.icon

  useReportTabProject(resolvedProjectPath)

  <main class={styles.root}>
    <header class={styles.header}>
      <div class={styles.headerIdentity}>
        <p class={styles.headerRepository}>
          {chat.repositoryLabel}
        </p>
        <h1 class={styles.headerTitle}>
          {chat.title}
        </h1>
      </div>
      <div
        aria-label={`${status.label}: ${status.description}`}
        class={styles.headerStatus}
        data-tone={status.tone}
        title={status.description}
      >
        <StatusIcon size={15} strokeWidth={2.2} />
        <span class={styles.headerStatusLabel}>
          {status.label}
        </span>
      </div>
    </header>
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
